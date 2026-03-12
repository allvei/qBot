//! ANSI color codes for terminal output

/// ANSI color codes
pub struct Color;

impl Color {
    // Log level colors
    pub const RESET: &'static str = "\x1b[0m";
    pub const WHITE: &'static str = "\x1b[37m";
    pub const ORANGE: &'static str = "\x1b[38;5;208m";
    pub const RED: &'static str = "\x1b[31m";
    
    // Semantic element colors
    pub const GUILD: &'static str = "\x1b[38;5;33m";      // Bright blue
    pub const CATEGORY: &'static str = "\x1b[38;5;141m";  // Purple
    pub const FORMAT: &'static str = "\x1b[38;5;228m";    // Yellow
    pub const USER: &'static str = "\x1b[38;5;120m";      // Green
    
    // Additional utility colors
    pub const CYAN: &'static str = "\x1b[36m";
    pub const GRAY: &'static str = "\x1b[90m";
    pub const BOLD: &'static str = "\x1b[1m";
}

/// Color a guild name
pub fn guild(name: &str) -> String {
    format!("{}{}{}", Color::GUILD, name, Color::RESET)
}

/// Color a category name
pub fn category(name: &str) -> String {
    format!("{}{}{}", Color::CATEGORY, name, Color::RESET)
}

/// Color a format name
pub fn format(name: &str) -> String {
    format!("{}{}{}", Color::FORMAT, name, Color::RESET)
}

/// Color a user name
pub fn user(name: &str) -> String {
    format!("{}{}{}", Color::USER, name, Color::RESET)
}

/// Color text with a custom color
pub fn custom(text: &str, color: &str) -> String {
    format!("{}{}{}", color, text, Color::RESET)
}

/// Color text orange (for warnings)
pub fn orange(text: &str) -> String {
    format!("{}{}{}", Color::ORANGE, text, Color::RESET)
}

/// Color text red (for errors)
pub fn red(text: &str) -> String {
    format!("{}{}{}", Color::RED, text, Color::RESET)
}

/// Color text gray (for secondary info)
pub fn gray(text: &str) -> String {
    format!("{}{}{}", Color::GRAY, text, Color::RESET)
}

/// Make text bold
pub fn bold(text: &str) -> String {
    format!("{}{}{}", Color::BOLD, text, Color::RESET)
}
