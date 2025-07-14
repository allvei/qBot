// CHECK ME
#![allow(clippy::missing_docs_in_private_items)]

// Library crate for testable modules
pub mod database;
pub mod discord;
pub mod error;
pub mod events;
pub mod handlers;
pub mod models;

#[cfg(test)]
pub mod tests;

// Re-export
pub use database::Database;
pub use discord::*;
pub use events::*;
pub use handlers::{admin, queue, session};
pub use models::*;
