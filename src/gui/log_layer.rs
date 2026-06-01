//! Custom tracing layer to capture logs for GUI display

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::Subscriber;
use tracing_subscriber::Layer;

/// Custom tracing layer that captures log events to a ring buffer
pub struct GuiLogLayer {
  log_buffer: Arc<Mutex<VecDeque<String>>>,
  max_lines: usize,
}

impl GuiLogLayer {
  pub fn new(log_buffer: Arc<Mutex<VecDeque<String>>>, max_lines: usize) -> Self {
    Self { log_buffer, max_lines }
  }
}

impl<S> Layer<S> for GuiLogLayer
where
  S: Subscriber,
{
  fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
    // Extract metadata
    let metadata = event.metadata();
    let level = metadata.level();
    let target = metadata.target();

    // Only capture qbot logs
    if !target.starts_with("qbot") {
      return;
    }

    // Format the log line with file and line info
    let mut message = String::new();
    message.push_str(&format!("[{}] ", level));
    
    // Add file and line number if available
    if let Some(file) = metadata.file() {
      if let Some(line) = metadata.line() {
        message.push_str(&format!("{}:{} ", file, line));
      }
    }
    
    // Add target (module path)
    message.push_str(&format!("{}: ", target));

    // Try to extract the message
    let mut visitor = MessageVisitor::new(&mut message);
    event.record(&mut visitor);

    // Add to buffer
    if let Ok(mut buffer) = self.log_buffer.try_lock() {
      if buffer.len() >= self.max_lines {
        buffer.pop_front();
      }
      buffer.push_back(message);
    }
  }
}

/// Visitor to extract the message from a tracing event
struct MessageVisitor<'a> {
  message: &'a mut String,
}

impl<'a> MessageVisitor<'a> {
  fn new(message: &'a mut String) -> Self {
    Self { message }
  }
}

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
  fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
    if field.name() == "message" {
      self.message.push_str(&format!("{:?}", value));
    }
  }

  fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
    if field.name() == "message" {
      self.message.push_str(value);
    }
  }
}
