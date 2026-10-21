//! The output thread. It only moves ready-made byte buffers to the sink:
//! no decoding, allocation, file I/O or logging happens here.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::player::{Inbox, Shared};
use crate::sink::{AudioSink, OutputFormat, SinkError, StreamFormat};

/// How often a paused writer re-checks for resume, seek or stop.
const PAUSE_POLL: Duration = Duration::from_millis(20);

/// A block of packed audio, ready for the device.
#[derive(Debug)]
pub(crate) struct Chunk {
    /// Playback generation; bumped by seek and stop to invalidate queued audio.
    pub generation: u64,
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
    /// First audio of this generation reached the device.
    Started {
        generation: u64,
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

/// What the writer knows about the device.
#[derive(Default)]
struct Device {
    open: bool,
    /// Generation whose audio the device holds.
    generation: u64,
    /// Audio of `generation` was written.
    started: bool,
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
                        generation,
                        ..Device::default()
                    };
                    self.send(WriterEvent::Opened { generation, result });
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
                            Ok(()) => WriterEvent::Drained { generation },
                            Err(error) => WriterEvent::Failed { generation, error },
                        };
                        self.send(event);
                    }
                }
                ToWriter::Close => {
                    if device.open {
                        self.sink.close();
                    }
                    device = Device::default();
                }
            }
        }
        if device.open {
            self.sink.close();
        }
    }

    fn play(&mut self, chunk: &Chunk, device: &mut Device) -> Result<(), SinkError> {
        if chunk.generation != device.generation {
            // First chunk after a seek: drop what the device still holds.
            self.sink.discard()?;
            device.generation = chunk.generation;
            device.started = false;
        }
        self.wait_while_paused(chunk.generation, device)?;
        if !self.is_live(chunk.generation) {
            return Ok(());
        }
        self.sink.write(&chunk.bytes)?;
        self.shared
            .position
            .store(chunk.start_frame + chunk.frames, Ordering::Release);
        if !device.started {
            device.started = true;
            self.send(WriterEvent::Started {
                generation: chunk.generation,
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
