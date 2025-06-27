use serde::{Deserialize, Serialize};

// #[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    pub queue: u64,
    pub buffer: u64,
    pub red: u64,
    pub blu: u64,
}

impl Channels {
    pub fn new(queue: u64, buffer: u64, red: u64, blu: u64) -> Self {
        Self { queue, buffer, red, blu }
    }
}