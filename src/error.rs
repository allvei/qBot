//! # Error Module
//!
//! This module defines custom error types for the application.
//! It uses the `thiserror` crate to derive error implementations.

use thiserror::Error;

/// Application-specific error types
#[derive(Debug, Error)]
pub enum AppError {
    /// Error related to player operations
    #[error("Player error: {0}")]
    PlayerError(String),

    /// Error related to session operations
    #[error("Session error: {0}")]
    SessionError(String),

    /// Error related to group operations
    #[error("Group error: {0}")]
    GroupError(String),

    /// Error related to database operations
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// Error related to Discord API operations
    #[error("Discord API error: {0}")]
    DiscordError(String),

    /// Error related to configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Generic application error
    #[error("Application error: {0}")]
    GenericError(String),
}

/// Result type alias for application operations
pub type AppResult<T> = Result<T, AppError>;

/// Extension trait for adding context to errors
pub trait ResultExt<T, E> {
    /// Add context to an error
    fn with_context<C, F>(
        self,
        context: F,
    ) -> Result<T, AppError>
    where
        F: FnOnce() -> C,
        C: std::fmt::Display;
}

impl<T, E: std::error::Error + 'static> ResultExt<T, E> for Result<T, E> {
    fn with_context<C, F>(
        self,
        context: F,
    ) -> Result<T, AppError>
    where
        F: FnOnce() -> C,
        C: std::fmt::Display,
    {
        self.map_err(|e| AppError::GenericError(format!("{}: {}", context(), e)))
    }
}

/// Extension trait for converting string errors to AppError
pub trait StringErrorExt<T> {
    /// Convert a string error to an AppError
    fn to_app_error(
        self,
        error_type: fn(String) -> AppError,
    ) -> Result<T, AppError>;
}

impl<T> StringErrorExt<T> for Result<T, String> {
    fn to_app_error(
        self,
        error_type: fn(String) -> AppError,
    ) -> Result<T, AppError> {
        self.map_err(error_type)
    }
}
