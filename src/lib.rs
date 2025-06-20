// Library crate for testable modules
pub mod database;
pub mod models;
pub mod handlers;

// Re-export for easier testing
pub use database::Database;

// Re-export models
pub use models::{user::User, session_model::Session, session_model::SessionStatus, queue::QueueSession, queue::QueueType};

// Re-export handlers
pub use handlers::queue;
pub use handlers::admin;
pub use handlers::session_handler;
