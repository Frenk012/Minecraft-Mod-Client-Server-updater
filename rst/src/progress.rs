use std::sync::{Arc, Mutex};

/// Shared status line: background tasks write, GUI reads each frame.
#[derive(Clone, Default)]
pub struct Progress(Arc<Mutex<String>>);

impl Progress {
    pub fn set(&self, msg: impl Into<String>) {
        *self.0.lock().unwrap() = msg.into();
    }

    pub fn get(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}
