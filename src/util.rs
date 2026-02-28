//! Utility functions for common operations throughout the codebase

use chrono::Utc;

/// Application configuration loaded from environment variables
pub struct Config {
    pub token: String,
    pub database_url: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn load() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        
        let token = std::env::var("DISCORD_TOKEN")
            .map_err(|_| anyhow::anyhow!("DISCORD_TOKEN environment variable is required"))?;
        
        let db_file = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "./pf_pug_bot.db".to_string());
        let database_url = format!("sqlite:{db_file}");
        
        Ok(Self { token, database_url })
    }
}

/// Initialize tracing with both console and file output
pub fn init_logging() {
    use std::fs;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    
    // Create logs directory if it doesn't exist
    if let Err(e) = fs::create_dir_all("logs") {
        eprintln!("Failed to create logs directory: {}", e);
    }
    
    let timer = tracing_subscriber::fmt::time::UtcTime::new(
        time::format_description::parse("[hour]:[minute]:[second]").unwrap()
    );
    
    // Console layer (existing behavior)
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(true)
        .with_target(false)
        .with_timer(timer.clone())
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true)
        .with_level(false)
        .compact();
    
    // File layer with more detailed information
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false) // No colors in file
        .with_target(true) // Include target in file
        .with_timer(timer)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true)
        .with_level(true) // Include log level in file
        .with_writer(
            tracing_appender::rolling::daily("logs", "qbot.log")
        )
        .json(); // Use structured JSON format for file logs
    
    // Initialize subscriber with both layers
    tracing_subscriber::registry()
        .with(console_layer)
        .with(file_layer)
        .init();
}

/// Discord timestamp styles
#[derive(Debug, Clone, Copy)]
pub enum Style {
    /// Short time (e.g., "15:32")
    Short,
    /// Long time (e.g., "15:32:45")
    Long,
    /// Short date (e.g., "21/02/2026")
    ShortDate,
    /// Long date (e.g., "21 February 2026")
    LongDate,
    /// Short date/time (e.g., "21/02/2026 15:32")
    ShortDateTime,
    /// Long date/time (e.g., "21 February 2026 15:32")
    LongDateTime,
    /// Relative time (e.g., "2 hours ago")
    Relative,
}

impl Style {
    /// Get the Discord timestamp format character
    pub fn as_char(self) -> &'static str {
        match self {
            Style::Short => "t",
            Style::Long => "T",
            Style::ShortDate => "d",
            Style::LongDate => "D",
            Style::ShortDateTime => "f",
            Style::LongDateTime => "F",
            Style::Relative => "R",
        }
    }
}

/// Create a Discord timestamp for the current time
pub fn now(style: Style) -> String {
    format!("<t:{}:{}>", Utc::now().timestamp(), style.as_char())
}

/// Create a Discord timestamp for a specific Unix timestamp
pub fn timestamp_from_unix(timestamp: i64, style: Style) -> String {
    format!("<t:{}:{}>", timestamp, style.as_char())
}

/// Create a Discord timestamp for a specific chrono::DateTime
pub fn timestamp_from_datetime(datetime: &chrono::DateTime<chrono::Utc>, style: Style) -> String {
    format!("<t:{}:{}>", datetime.timestamp(), style.as_char())
}

/// Create a Discord timestamp for a SystemTime
pub fn timestamp_from_system_time(system_time: &std::time::SystemTime, style: Style) -> Option<String> {
    system_time
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| format!("<t:{}:{}>", duration.as_secs(), style.as_char()))
}

/// Create a Discord timestamp for a future time (current time + duration)
pub fn future_timestamp(duration_secs: u64, style: Style) -> String {
    let future_timestamp = Utc::now().timestamp() + duration_secs as i64;
    format!("<t:{}:{}>", future_timestamp, style.as_char())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timestamp_formats() {
        let now = Utc::now().timestamp();
        
        assert_eq!(crate::now(Style::ShortDateTime), format!("<t:{}:f>", now));
        assert_eq!(crate::now(Style::Relative), format!("<t:{}:R>", now));
        assert_eq!(timestamp_from_unix(1234567890, Style::LongDateTime), "<t:1234567890:F>");
    }

    #[test]
    fn test_timestamp_styles() {
        assert_eq!(Style::Short.as_char(), "t");
        assert_eq!(Style::Long.as_char(), "T");
        assert_eq!(Style::ShortDate.as_char(), "d");
        assert_eq!(Style::LongDate.as_char(), "D");
        assert_eq!(Style::ShortDateTime.as_char(), "f");
        assert_eq!(Style::LongDateTime.as_char(), "F");
        assert_eq!(Style::Relative.as_char(), "R");
    }
}
