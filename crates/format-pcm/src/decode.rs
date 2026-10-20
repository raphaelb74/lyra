use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;

use symphonia::core::audio::{Audio, AudioBuffer, GenericAudioBufferRef, sample::Sample};
use symphonia::core::codecs::audio::well_known::CODEC_ID_FLAC;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::{Error as SymphoniaError, SeekErrorKind};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

/// Most channels accepted. Stereo DACs are the target; more channels need an
/// explicit channel map, which is not implemented.
const MAX_CHANNELS: usize = 2;

/// Source codec, for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Flac,
    Wav,
}

/// Properties of the decoded stream. Playback must open the DAC with exactly
/// these, or refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    pub codec: Codec,
    pub sample_rate: u32,
    pub channels: usize,
    /// Significant bits per sample (16, 24, …), at most 32.
    pub bits_per_sample: u32,
    /// Length in frames, when the container states it.
    pub total_frames: Option<u64>,
}

#[derive(Debug)]
pub enum DecodeError {
    Io(io::Error),
    /// Valid file this decoder will not play bit-perfectly (float, unsigned,
    /// multichannel, unknown codec).
    Unsupported(&'static str),
    /// Corrupt or truncated data.
    Malformed(String),
    SeekOutOfRange,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read file: {error}"),
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::Malformed(what) => write!(f, "invalid audio data: {what}"),
            Self::SeekOutOfRange => f.write_str("seek position is past the end of the track"),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SymphoniaError> for DecodeError {
    fn from(error: SymphoniaError) -> Self {
        match error {
            SymphoniaError::IoError(error) => Self::Io(error),
            SymphoniaError::Unsupported(what) => Self::Unsupported(what),
            SymphoniaError::SeekError(SeekErrorKind::OutOfRange) => Self::SeekOutOfRange,
            SymphoniaError::ResetRequired => {
                Self::Unsupported("stream parameters change in the middle of the track")
            }
            other => Self::Malformed(other.to_string()),
        }
    }
}

/// Adapts any `Read + Seek` to Symphonia; the length is unknown.
struct ReadSeek<R>(R);

impl<R: io::Read> io::Read for ReadSeek<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl<R: io::Seek> io::Seek for ReadSeek<R> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl<R: io::Read + io::Seek + Send + Sync> MediaSource for ReadSeek<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

/// Decodes one audio track to left-justified interleaved `i32` samples.
pub struct Decoder {
    reader: Box<dyn FormatReader>,
    codec: Box<dyn AudioDecoder>,
    track_id: u32,
    info: StreamInfo,
    /// Frames to drop from the next packet after an accurate seek.
    skip_frames: u64,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoder")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl Decoder {
    /// Opens a WAV or FLAC file.
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let file = File::open(path).map_err(DecodeError::Io)?;
        let extension = path.extension().and_then(|extension| extension.to_str());
        Self::from_reader(file, extension)
    }

    /// Opens any seekable reader. `extension` (`"flac"`, `"wav"`) speeds up
    /// format detection; content is probed either way.
    pub fn from_reader<R>(reader: R, extension: Option<&str>) -> Result<Self, DecodeError>
    where
        R: io::Read + io::Seek + Send + Sync + 'static,
    {
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let source: Box<dyn MediaSource> = Box::new(ReadSeek(reader));
        let stream = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
        let reader = symphonia::default::get_probe().probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or(DecodeError::Unsupported("no audio track"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(DecodeError::Unsupported("no audio codec parameters"))?;

        let sample_rate = params
            .sample_rate
            .ok_or(DecodeError::Malformed("sample rate missing".into()))?;
        let channels = params
            .channels
            .as_ref()
            .map(symphonia::core::audio::Channels::count)
            .ok_or(DecodeError::Malformed("channel layout missing".into()))?;
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(DecodeError::Unsupported(
                "only mono and stereo are supported",
            ));
        }
        let bits_per_sample = params.bits_per_sample.ok_or(DecodeError::Unsupported(
            "bit depth not stated (float or compressed PCM)",
        ))?;
        if !(8..=32).contains(&bits_per_sample) {
            return Err(DecodeError::Unsupported("bit depth outside 8–32"));
        }
        // Seeking and skipping count in frames, so timestamps must be frames.
        if let Some(time_base) = track.time_base
            && (time_base.numer.get() != 1 || time_base.denom.get() != sample_rate)
        {
            return Err(DecodeError::Unsupported(
                "timestamps are not in sample frames",
            ));
        }

        let codec = if params.codec == CODEC_ID_FLAC {
            Codec::Flac
        } else {
            Codec::Wav
        };
        let info = StreamInfo {
            codec,
            sample_rate,
            channels,
            bits_per_sample,
            total_frames: track.num_frames,
        };
        let track_id = track.id;
        // Verification checks FLAC's embedded MD5 of the original audio.
        let options = AudioDecoderOptions::default().verify(true);
        let codec = symphonia::default::get_codecs().make_audio_decoder(params, &options)?;
        Ok(Self {
            reader,
            codec,
            track_id,
            info,
            skip_frames: 0,
        })
    }

    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    /// Replaces `out` with the next block of interleaved samples. Returns
    /// `false` at the end of the track, with `out` empty.
    pub fn decode_next(&mut self, out: &mut Vec<i32>) -> Result<bool, DecodeError> {
        out.clear();
        while out.is_empty() {
            let Some(packet) = self.reader.next_packet()? else {
                return Ok(false);
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buffer = self.codec.decode(&packet)?;
            let skip = usize::try_from(self.skip_frames).unwrap_or(usize::MAX);
            let frames = buffer.frames();
            let used = append_interleaved(&buffer, skip, self.info.channels, out)?;
            let skipped = u64::try_from(frames - used).unwrap_or(u64::MAX);
            self.skip_frames = self.skip_frames.saturating_sub(skipped);
        }
        Ok(true)
    }

    /// Moves to `frame`. The next [`Self::decode_next`] starts exactly there.
    pub fn seek(&mut self, frame: u64) -> Result<(), DecodeError> {
        let ts = i64::try_from(frame).map_err(|_| DecodeError::SeekOutOfRange)?;
        let seeked = self.reader.seek(
            SeekMode::Accurate,
            SeekTo::Timestamp {
                ts: Timestamp::new(ts),
                track_id: self.track_id,
            },
        )?;
        self.codec.reset();
        let behind = seeked.required_ts.get() - seeked.actual_ts.get();
        self.skip_frames = u64::try_from(behind).unwrap_or(0);
        Ok(())
    }

    /// After the last block: `Some(true)` if the decoded audio matches the
    /// checksum stored in the file (FLAC MD5), `None` if there is none.
    /// Only meaningful for a full decode without seeking.
    pub fn verify(&mut self) -> Option<bool> {
        self.codec.finalize().verify_ok
    }
}

/// Appends the buffer's frames after `skip` to `out` as left-justified
/// interleaved `i32`. Returns how many frames were appended.
fn append_interleaved(
    buffer: &GenericAudioBufferRef<'_>,
    skip: usize,
    channels: usize,
    out: &mut Vec<i32>,
) -> Result<usize, DecodeError> {
    match *buffer {
        GenericAudioBufferRef::S16(buffer) => interleave(buffer, skip, channels, out, |sample| {
            i32::from(sample) << 16
        }),
        GenericAudioBufferRef::S24(buffer) => {
            interleave(buffer, skip, channels, out, |sample| sample.inner() << 8)
        }
        GenericAudioBufferRef::S32(buffer) => interleave(buffer, skip, channels, out, |s| s),
        GenericAudioBufferRef::F32(_) | GenericAudioBufferRef::F64(_) => Err(
            DecodeError::Unsupported("floating-point PCM cannot be played bit-perfectly"),
        ),
        _ => Err(DecodeError::Unsupported(
            "only signed integer PCM is supported",
        )),
    }
}

fn interleave<S: Sample>(
    buffer: &AudioBuffer<S>,
    skip: usize,
    channels: usize,
    out: &mut Vec<i32>,
    to_i32: impl Fn(S) -> i32,
) -> Result<usize, DecodeError> {
    if buffer.num_planes() != channels {
        return Err(DecodeError::Unsupported(
            "channel count changes within the track",
        ));
    }
    let frames = buffer.frames().saturating_sub(skip);
    let start = out.len();
    out.resize(start + frames * channels, 0);
    for channel in 0..channels {
        let plane = buffer
            .plane(channel)
            .and_then(|plane| plane.get(skip..))
            .ok_or(DecodeError::Malformed(
                "decoder returned a short channel".into(),
            ))?;
        for (frame, &sample) in out[start..].chunks_exact_mut(channels).zip(plane) {
            frame[channel] = to_i32(sample);
        }
    }
    Ok(frames)
}
