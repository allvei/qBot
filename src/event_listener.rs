use std::sync::Arc;

pub struct EventListener {}

impl EventListener {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}
