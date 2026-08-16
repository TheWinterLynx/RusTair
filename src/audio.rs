use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

/// Native sound engine shared by the Altair and ASR-33. Failure to open an
/// audio device is deliberately non-fatal so CI/headless builds still work.
pub struct AudioEngine {
    stream: Option<OutputStream>,
    loops: HashMap<String, Sink>,
    muted: bool,
}

impl Default for AudioEngine {
    fn default() -> Self { Self::new() }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            stream: OutputStreamBuilder::open_default_stream().ok(),
            loops: HashMap::new(),
            muted: false,
        }
    }

    pub fn available(&self) -> bool { self.stream.is_some() }
    pub fn muted(&self) -> bool { self.muted }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted { self.stop_all_loops(); }
    }

    pub fn play_once(&self, path: impl AsRef<Path>) {
        if self.muted { return; }
        let Some(stream) = &self.stream else { return };
        let Ok(file) = File::open(path) else { return };
        let Ok(source) = Decoder::try_from(file) else { return };
        let sink = Sink::connect_new(stream.mixer());
        sink.append(source);
        sink.detach();
    }

    pub fn start_loop(&mut self, name: &str, path: impl AsRef<Path>) {
        if self.muted || self.loops.contains_key(name) { return; }
        let Some(stream) = &self.stream else { return };
        let Ok(file) = File::open(path) else { return };
        let Ok(source) = Decoder::try_from(file) else { return };
        let sink = Sink::connect_new(stream.mixer());
        sink.append(source.repeat_infinite());
        self.loops.insert(name.to_owned(), sink);
    }

    pub fn stop_loop(&mut self, name: &str) {
        if let Some(sink) = self.loops.remove(name) { sink.stop(); }
    }

    pub fn stop_all_loops(&mut self) {
        for (_, sink) in self.loops.drain() { sink.stop(); }
    }
}
