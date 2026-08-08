//! Sound effects (button clicks etc.) via rodio.
//!
//! Keeps the output stream alive for the app lifetime and plays short ogg
//! clips loaded from the `assets` directory. Failures are silent — audio is
//! a nice-to-have, never fatal.

use std::io::Cursor;
use std::path::Path;

pub struct Audio {
    _stream: Option<rodio::OutputStream>,
    handle: Option<rodio::OutputStreamHandle>,
    button: Option<Vec<u8>>,
    button_large: Option<Vec<u8>>,
    ending: Option<Vec<u8>>,
    enabled: bool,
}

impl Audio {
    pub fn new(assets_dir: &Path) -> Self {
        let (stream, handle) = match rodio::OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(_) => (None, None),
        };
        let load = |name: &str| std::fs::read(assets_dir.join(name)).ok();
        Self {
            _stream: stream,
            handle,
            button: load("button.ogg"),
            button_large: load("button_large.ogg"),
            ending: load("ending.ogg"),
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn play_bytes(&self, bytes: &Option<Vec<u8>>) {
        if !self.enabled {
            return;
        }
        let (Some(handle), Some(data)) = (&self.handle, bytes) else {
            return;
        };
        if let Ok(decoder) = rodio::Decoder::new(Cursor::new(data.clone())) {
            let _ = handle.play_raw(rodio::source::Source::convert_samples(decoder));
        }
    }

    /// Small click for regular buttons.
    pub fn click(&self) {
        self.play_bytes(&self.button);
    }

    /// Bigger click for primary actions (download / launch).
    pub fn click_large(&self) {
        self.play_bytes(&self.button_large);
    }

    /// Celebratory sound after an install / launch completes.
    pub fn ending(&self) {
        self.play_bytes(&self.ending);
    }
}
