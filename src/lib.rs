// CHECK ME
#![allow(clippy::missing_docs_in_private_items)]

// Library crate for testable modules
pub mod database;
pub mod error;
pub mod handlers;
pub mod models;

// Re-export
pub use database::*;
pub use handlers::{admin, player};
pub use models::*;
