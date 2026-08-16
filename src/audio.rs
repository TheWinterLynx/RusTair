use std::fs::File;
use std::path::Path;

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

/// Small native sound engine.  Failure to open an audio device is deliberately
/// non-fatal: the emulator remains usable on headless machines and CI runners.
pub struct AudioEngine {
    stream: Option<OutputStream>,
    loop_sink: Option<Sink>,
    muted: bool,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let stream = OutputStreamBuilder::open_default_stream().ok();
        Self {
            stream,
            loop_sink: None,
            muted: false,
        }
    }

    pub fn available(&self) -> bool {
        self.stream.is_some()
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted {
            self.stop_loop();
        }
    }

    pub fn play_once(&self, path: impl AsRef<Path>) {
        if self.muted {
            return;
        }
        let Some(stream) = &self.stream else { return };
        let Ok(file) = File::open(path) else { return };
        let Ok(source) = Decoder::try_from(file) else { return };

        let sink = Sink::connect_new(stream.mixer());
        sink.append(source);
        // Keep playing after the temporary Sink handle leaves this function.
        sink.detach();
    }

    pub fn start_loop(&mut self, path: impl AsRef<Path>) {
        if self.muted || self.loop_sink.is_some() {
            return;
        }
        let Some(stream) = &self.stream else { return };
        let Ok(file) = File::open(path) else { return };
        let Ok(source) = Decoder::try_from(file) else { return };

        let sink = Sink::connect_new(stream.mixer());
        sink.append(source.repeat_infinite());
        self.loop_sink = Some(sink);
    }

    pub fn stop_loop(&mut self) {
        if let Some(sink) = self.loop_sink.take() {
            sink.stop();
        }
    }
}
