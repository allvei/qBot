// CHECK ME
#![allow(clippy::missing_docs_in_private_items)]

// Library crate for testable modules
pub mod database;
pub mod handlers;
pub mod models;

// Re-export for easier testing
pub use database::Database;

// Re-export models
pub use models::*;

// Re-export handlers
pub use handlers::admin;
pub use handlers::queue;
pub use handlers::session;
