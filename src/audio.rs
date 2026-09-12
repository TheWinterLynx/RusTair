use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};

use crate::embedded_assets;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AudioDomain {
    Altair,
    Asr33,
}

struct ActiveLoop {
    sink: Sink,
    domain: AudioDomain,
}

/// Native sound engine shared by the Altair and ASR-33. Failure to open an
/// audio device is deliberately non-fatal so CI/headless builds still work.
///
/// One host output stream/mixer is shared, while the Altair chassis and ASR-33
/// retain independent mute domains. This avoids opening multiple host devices
/// merely to let the operator silence one physical machine without muting the
/// other.
///
/// Audio data is compiled into the executable; callers keep using stable asset
/// names so the rest of the application does not need to know where the bytes
/// come from.
pub struct AudioEngine {
    stream: Option<OutputStream>,
    loops: HashMap<String, ActiveLoop>,
    altair_muted: bool,
    asr33_muted: bool,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        let mut stream = OutputStreamBuilder::open_default_stream().ok();
        if let Some(stream) = stream.as_mut() {
            // Rodio prints a diagnostic every time OutputStream is dropped,
            // including the normal application shutdown path. RusTair owns the
            // stream for the lifetime of AudioEngine, so that message is noise,
            // not an indication of premature audio teardown.
            stream.log_on_drop(false);
        }
        Self {
            stream,
            loops: HashMap::new(),
            altair_muted: false,
            asr33_muted: false,
        }
    }

    pub fn available(&self) -> bool {
        self.stream.is_some()
    }
    pub fn altair_muted(&self) -> bool {
        self.altair_muted
    }
    pub fn asr33_muted(&self) -> bool {
        self.asr33_muted
    }

    pub fn set_altair_muted(&mut self, muted: bool) {
        if self.altair_muted == muted {
            return;
        }
        self.altair_muted = muted;
        self.set_domain_loop_volume(AudioDomain::Altair, if muted { 0.0 } else { 1.0 });
    }

    pub fn set_asr33_muted(&mut self, muted: bool) {
        if self.asr33_muted == muted {
            return;
        }
        self.asr33_muted = muted;
        self.set_domain_loop_volume(AudioDomain::Asr33, if muted { 0.0 } else { 1.0 });
    }

    fn domain_muted(&self, domain: AudioDomain) -> bool {
        match domain {
            AudioDomain::Altair => self.altair_muted,
            AudioDomain::Asr33 => self.asr33_muted,
        }
    }

    fn play_once_for(&self, domain: AudioDomain, path: impl AsRef<Path>) {
        if self.domain_muted(domain) {
            return;
        }
        let Some(stream) = &self.stream else { return };
        let Some(path) = path.as_ref().to_str() else {
            return;
        };
        let Some(bytes) = embedded_assets::get(path) else {
            return;
        };
        let Ok(source) = Decoder::try_from(Cursor::new(bytes)) else {
            return;
        };
        let sink = Sink::connect_new(stream.mixer());
        sink.append(source);
        sink.detach();
    }

    /// Play an Altair/chassis sound. This remains the default domain so existing
    /// front-panel call sites cannot accidentally become ASR-33 audio.
    pub fn play_once(&self, path: impl AsRef<Path>) {
        self.play_once_for(AudioDomain::Altair, path);
    }

    pub fn play_asr_once(&self, path: impl AsRef<Path>) {
        self.play_once_for(AudioDomain::Asr33, path);
    }

    fn start_loop_for(&mut self, domain: AudioDomain, name: &str, path: impl AsRef<Path>) {
        if self.loops.contains_key(name) {
            return;
        }
        let Some(stream) = &self.stream else { return };
        let Some(path) = path.as_ref().to_str() else {
            return;
        };
        let Some(bytes) = embedded_assets::get(path) else {
            return;
        };
        let Ok(source) = Decoder::try_from(Cursor::new(bytes)) else {
            return;
        };
        let sink = Sink::connect_new(stream.mixer());
        if self.domain_muted(domain) {
            sink.set_volume(0.0);
        }
        sink.append(source.repeat_infinite());
        self.loops
            .insert(name.to_owned(), ActiveLoop { sink, domain });
    }

    pub fn start_loop(&mut self, name: &str, path: impl AsRef<Path>) {
        self.start_loop_for(AudioDomain::Altair, name, path);
    }

    pub fn start_asr_loop(&mut self, name: &str, path: impl AsRef<Path>) {
        self.start_loop_for(AudioDomain::Asr33, name, path);
    }

    pub fn stop_loop(&mut self, name: &str) {
        if let Some(active) = self.loops.remove(name) {
            active.sink.stop();
        }
    }

    fn set_domain_loop_volume(&mut self, domain: AudioDomain, volume: f32) {
        for active in self.loops.values() {
            if active.domain == domain {
                active.sink.set_volume(volume);
            }
        }
    }

    pub fn stop_all_loops(&mut self) {
        for (_, active) in self.loops.drain() {
            active.sink.stop();
        }
    }
}
