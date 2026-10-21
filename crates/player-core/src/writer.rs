//! The output thread. It only moves ready-made byte buffers to the sink:
//! no decoding, file I/O or logging happens here, and no allocation while
//! audio streams (only for the silence around it, before playback starts
//! and after it stops).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::player::{Inbox, Shared};
use crate::sink::{AudioSink, Encoding, OutputFormat, SinkError, StreamFormat};

/// How often a paused writer re-checks for resume, seek or stop.
const PAUSE_POLL: Duration = Duration::from_millis(20);

/// A block of packed audio, ready for the device.
#[derive(Debug)]
pub(crate) struct Chunk {
    /// Playback generation; bumped by seek and stop to invalidate queued audio.
    pub generation: u64,
    /// Which track of the queue this is (a counter, unique per start).
    pub seq: u64,
    /// Track position of the first frame.
    pub start_frame: u64,
    pub frames: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum ToWriter {
    Open {
        generation: u64,
        device: String,
        stream: StreamFormat,
    },
    /// Silence before the first audio, so the DAC settles in the new
    /// format. Sent by the controller once it accepted the output.
    Preroll {
        generation: u64,
        duration: Duration,
    },
    Chunk(Chunk),
    /// End of track: play out what is queued.
    Drain {
        generation: u64,
    },
    Close,
}

#[derive(Debug)]
pub(crate) enum WriterEvent {
    Opened {
        generation: u64,
        result: Result<OutputFormat, SinkError>,
    },
    /// The first audio of track `seq` reached the device: the first track
    /// of a generation, or the next one of a gapless sequence.
    Started {
        generation: u64,
        seq: u64,
    },
    Drained {
        generation: u64,
    },
    Failed {
        generation: u64,
        error: SinkError,
    },
}

pub(crate) struct Writer<S> {
    pub sink: S,
    pub input: Receiver<ToWriter>,
    /// Empty buffers go back to the controller.
    pub free: Sender<Vec<u8>>,
    pub events: Sender<Inbox>,
    pub shared: Arc<Shared>,
}

/// DSD silence played before the device closes in the middle of a DSD
/// stream, so the DAC does not stop on a cut-off stream.
const DSD_TAIL: Duration = Duration::from_millis(100);

/// What the writer knows about the device.
#[derive(Default)]
struct Device {
    open: bool,
    output: Option<OutputFormat>,
    /// Audio was written since the open and has not been drained.
    playing: bool,
    /// Generation whose audio the device holds.
    generation: u64,
    /// Track whose audio was written last, if any in this generation.
    seq: Option<u64>,
    paused: bool,
}

impl<S: AudioSink> Writer<S> {
    pub fn run(mut self) {
        let mut device = Device::default();
        while let Ok(message) = self.input.recv() {
            match message {
                ToWriter::Open {
                    generation,
                    device: name,
                    stream,
                } => {
                    let result = self.sink.open(&name, &stream);
                    device = Device {
                        open: result.is_ok(),
                        output: result.as_ref().ok().copied(),
                        generation,
                        ..Device::default()
                    };
                    self.send(WriterEvent::Opened { generation, result });
                }
                ToWriter::Preroll {
                    generation,
                    duration,
                } => {
                    if self.is_live(generation)
                        && let Some(output) = device.output
                        && let Err(error) = self.silence(output, duration)
                    {
                        self.send(WriterEvent::Failed { generation, error });
                    }
                }
                ToWriter::Chunk(chunk) => {
                    // Stale chunks (queued before a seek or stop) are skipped.
                    if self.is_live(chunk.generation)
                        && let Err(error) = self.play(&chunk, &mut device)
                    {
                        self.send(WriterEvent::Failed {
                            generation: chunk.generation,
                            error,
                        });
                    }
                    // A closed controller means shutdown; nothing to recycle into.
                    let _ = self.free.send(chunk.bytes);
                }
                ToWriter::Drain { generation } => {
                    let result = self.wait_while_paused(generation, &mut device);
                    if self.is_live(generation) {
                        let event = match result.and_then(|()| self.sink.drain()) {
                            Ok(()) => {
                                device.playing = false;
                                WriterEvent::Drained { generation }
                            }
                            Err(error) => WriterEvent::Failed { generation, error },
                        };
                        self.send(event);
                    }
                }
                ToWriter::Close => {
                    if device.open {
                        self.end_dsd(&device);
                        self.sink.close();
                    }
                    device = Device::default();
                }
            }
        }
        if device.open {
            self.end_dsd(&device);
            self.sink.close();
        }
    }

    /// Writes `duration` of silence in the device's format.
    fn silence(&mut self, output: OutputFormat, duration: Duration) -> Result<(), SinkError> {
        let frames =
            u64::from(output.sample_rate) * u64::try_from(duration.as_millis()).unwrap_or(0) / 1000;
        // Even, so DoP's marker phase is the same after it.
        let frames = usize::try_from(frames).unwrap_or(0) & !1;
        let mut bytes = Vec::new();
        output.encoding.silence(output.channels, frames, &mut bytes);
        self.sink.write(&bytes)
    }

    /// Stopped in the middle of DSD: drop the rest, play a little DSD
    /// silence and let it finish, so the DAC sees the stream end cleanly
    /// instead of being cut off mid-stream (some DACs then mishandle what
    /// comes next). Errors are moot: the device is closing.
    fn end_dsd(&mut self, device: &Device) {
        let Some(output) = device.output else { return };
        if !device.playing || !matches!(output.encoding, Encoding::Dsd(_) | Encoding::Dop(_)) {
            return;
        }
        let _ = self.sink.discard();
        if self.silence(output, DSD_TAIL).is_ok() {
            let _ = self.sink.drain();
        }
    }

    fn play(&mut self, chunk: &Chunk, device: &mut Device) -> Result<(), SinkError> {
        if chunk.generation != device.generation {
            // First chunk after a seek: drop what the device still holds.
            self.sink.discard()?;
            device.generation = chunk.generation;
            device.seq = None;
        }
        self.wait_while_paused(chunk.generation, device)?;
        if !self.is_live(chunk.generation) {
            return Ok(());
        }
        self.sink.write(&chunk.bytes)?;
        device.playing = true;
        self.shared
            .position
            .store(chunk.start_frame + chunk.frames, Ordering::Release);
        if device.seq != Some(chunk.seq) {
            device.seq = Some(chunk.seq);
            self.send(WriterEvent::Started {
                generation: chunk.generation,
                seq: chunk.seq,
            });
        }
        Ok(())
    }

    /// Mirrors the pause flag onto the sink and blocks while paused. Returns
    /// early when `generation` is stopped or seeked away.
    fn wait_while_paused(&mut self, generation: u64, device: &mut Device) -> Result<(), SinkError> {
        while self.is_live(generation) {
            let paused = self.shared.paused.load(Ordering::Acquire);
            if paused != device.paused {
                self.sink.set_paused(paused)?;
                device.paused = paused;
            }
            if !paused {
                break;
            }
            std::thread::park_timeout(PAUSE_POLL);
        }
        Ok(())
    }

    fn is_live(&self, generation: u64) -> bool {
        self.shared.generation.load(Ordering::Acquire) == generation
    }

    fn send(&self, event: WriterEvent) {
        // The controller only disappears at shutdown.
        let _ = self.events.send(Inbox::Writer(event));
    }
}
