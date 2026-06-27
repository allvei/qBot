//! Discord response helpers to reduce repetitive embed and response creation
//!
//! This module provides helper functions and extension methods for creating
//! common Discord responses, embeds, and interaction patterns.

use crate::models::embeds::Ephemeral;
use crate::models::CommandContext as CC;
use crate::{CYAN, GREEN, RED, YELLOW};
use anyhow::Result;
use serenity::all::{ComponentInteraction as CX, Context, CreateActionRow as CAR, CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};

/// Common embed creation helpers
pub struct EmbedHelpers;

impl EmbedHelpers {
  /// Create a success embed (green)
  pub fn success(title: &str, description: &str) -> CE {
    CE::new().title(title).description(description).color(GREEN)
  }

  /// Create an error embed (red)
  pub fn error(title: &str, description: &str) -> CE {
    CE::new().title(title).description(description).color(RED)
  }

  /// Create a warning embed (yellow)
  pub fn warning(title: &str, description: &str) -> CE {
    CE::new().title(title).description(description).color(YELLOW)
  }

  /// Create an info embed (blue/cyan)
  pub fn info(title: &str, description: &str) -> CE {
    CE::new().title(title).description(description).color(CYAN)
  }

  /// Create an embed with configurable color
  pub fn with_color(title: &str, description: &str, color: u32) -> CE {
    CE::new().title(title).description(description).color(color)
  }

  /// Add a footer to an embed
  pub fn with_footer(mut embed: CE, text: &str) -> CE {
    embed = embed.footer(serenity::all::CreateEmbedFooter::new(text));
    embed
  }
}

/// Extension trait for common response patterns on command contexts
pub trait ResponseExt {
  /// Send an ephemeral success message
  async fn reply_success(&self, title: &str, description: &str) -> Result<()>;

  /// Send an ephemeral error message
  async fn reply_error(&self, title: &str, description: &str) -> Result<()>;

  /// Send an ephemeral warning message
  async fn reply_warning(&self, title: &str, description: &str) -> Result<()>;

  /// Send an ephemeral info message
  async fn reply_info(&self, title: &str, description: &str) -> Result<()>;

  /// Send an ephemeral plain text message
  async fn reply_ephemeral(&self, message: &str) -> Result<()>;

  /// Send an embed with components
  async fn reply_embed_with_components(&self, embed: CE, components: Vec<CAR>) -> Result<()>;

  /// Send an error response for component interactions
  async fn send_component_error_response(&self, message: &str) -> Result<()>;

  /// Send a success response for component interactions
  async fn send_component_success_response(&self, message: &str) -> Result<()>;
}

/// Extension trait for component interaction responses
pub trait ComponentResponseExt {
  /// Update with a success embed
  async fn update_success(&self, title: &str, description: &str) -> Result<()>;

  /// Update with an error embed
  async fn update_error(&self, title: &str, description: &str) -> Result<()>;

  /// Update with a warning embed
  async fn update_warning(&self, title: &str, description: &str) -> Result<()>;

  /// Update with an info embed
  async fn update_info(&self, title: &str, description: &str) -> Result<()>;

  /// Update with plain text message
  async fn update_message(&self, message: &str) -> Result<()>;
}

/// Extension trait for common response patterns on command contexts
impl ResponseExt for CC<'_> {
  /// Send an ephemeral success message
  async fn reply_success(&self, title: &str, description: &str) -> Result<()> {
    let embed = EmbedHelpers::success(title, description);
    self.intax.create_response(&self.ctx.http, Ephemeral::send(embed)).await?;
    Ok(())
  }

  /// Send an ephemeral error message
  async fn reply_error(&self, title: &str, description: &str) -> Result<()> {
    let embed = EmbedHelpers::error(title, description);
    self.intax.create_response(&self.ctx.http, Ephemeral::send(embed)).await?;
    Ok(())
  }

  /// Send an ephemeral warning message
  async fn reply_warning(&self, title: &str, description: &str) -> Result<()> {
    let embed = EmbedHelpers::warning(title, description);
    self.intax.create_response(&self.ctx.http, Ephemeral::send(embed)).await?;
    Ok(())
  }

  /// Send an ephemeral info message
  async fn reply_info(&self, title: &str, description: &str) -> Result<()> {
    let embed = EmbedHelpers::info(title, description);
    self.intax.create_response(&self.ctx.http, Ephemeral::send(embed)).await?;
    Ok(())
  }

  /// Send an ephemeral plain text message
  async fn reply_ephemeral(&self, message: &str) -> Result<()> {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    self.intax.create_response(&self.ctx.http, response).await?;
    Ok(())
  }

  /// Send an embed with components
  async fn reply_embed_with_components(&self, embed: CE, components: Vec<CAR>) -> Result<()> {
    let response = CIR::Message(CIRM::new().embed(embed).components(components).ephemeral(true));
    self.intax.create_response(&self.ctx.http, response).await?;
    Ok(())
  }

  // Add these methods to ResponseExt trait
  async fn send_component_error_response(&self, message: &str) -> Result<()> {
    let embed = CE::new().title("Error").description(message).color(RED);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    self.intax.create_response(&self.ctx.http, response).await?;
    Ok(())
  }

  async fn send_component_success_response(&self, message: &str) -> Result<()> {
    let embed = CE::new().title("Success").description(message).color(GREEN);

    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    self.intax.create_response(&self.ctx.http, response).await?;
    Ok(())
  }
}

/// Extension trait for component interaction responses
impl ComponentResponseExt for CX {
  /// Update with a success embed
  async fn update_success(&self, title: &str, description: &str) -> Result<()> {
    let embed = EmbedHelpers::success(title, description);
    let _response = CIR::UpdateMessage(CIRM::new().embed(embed));
    // Note: This needs Context, which we don't have in this trait
    // Implementation would need to be done differently or pass Context
    todo!("ComponentResponseExt needs Context parameter")
  }

  /// Update with an error embed
  async fn update_error(&self, _title: &str, _description: &str) -> Result<()> {
    todo!("ComponentResponseExt needs Context parameter")
  }

  /// Update with a warning embed
  async fn update_warning(&self, _title: &str, _description: &str) -> Result<()> {
    todo!("ComponentResponseExt needs Context parameter")
  }

  /// Update with an info embed
  async fn update_info(&self, _title: &str, _description: &str) -> Result<()> {
    todo!("ComponentResponseExt needs Context parameter")
  }

  /// Update with plain text message
  async fn update_message(&self, _message: &str) -> Result<()> {
    todo!("ComponentResponseExt needs Context parameter")
  }
}

/// Helper functions for error handling in interactions
pub struct InteractionHelpers;

impl InteractionHelpers {
  /// Send error response for component interactions, logging if it fails
  pub async fn send_component_error(interaction: &CX, ctx: &Context, message: &str) {
    let response = CIR::Message(CIRM::new().content(message).ephemeral(true));
    if let Err(e) = interaction.create_response(&ctx.http, response).await {
      tracing::error!("Failed to send error response: {e}");
    }
  }

  /// Send error response with embed for component interactions
  pub async fn send_component_error_embed(interaction: &CX, ctx: &Context, message: &str) {
    let embed = EmbedHelpers::error("Error", message);
    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    if let Err(e) = interaction.create_response(&ctx.http, response).await {
      tracing::error!("Failed to send error response: {e}");
    }
  }

  /// Send success response with embed for component interactions
  pub async fn send_component_success_embed(interaction: &CX, ctx: &Context, message: &str) {
    let embed = EmbedHelpers::success("Success", message);
    let response = CIR::Message(CIRM::new().embed(embed).ephemeral(true));
    if let Err(e) = interaction.create_response(&ctx.http, response).await {
      tracing::error!("Failed to send success response: {e}");
    }
  }

  /// Create a standardized configuration updated embed
  pub fn config_updated(key: &str, value: &str) -> CE {
    EmbedHelpers::success("Config updated", &format!("Set `{}` = `{}`", key, value))
  }

  /// Create a standardized role updated embed
  pub fn role_updated(role_type: &str, role_id: u64) -> CE {
    EmbedHelpers::success("Role updated", &format!("Set {} role to <@&{}>", role_type.to_lowercase(), role_id))
  }

  /// Create a standardized permission denied embed
  pub fn permission_denied(role: &str) -> CE {
    EmbedHelpers::error("Permission denied", &format!("This command is reserved for {}s", role.to_lowercase()))
  }
}

/// Macro for creating consistent error responses
#[macro_export]
macro_rules! respond_error {
  ($context:expr, $title:expr, $description:expr) => {
    $context.reply_error($title, $description).await
  };
  ($context:expr, $description:expr) => {
    $context.reply_error("Error", $description).await
  };
}

/// Macro for creating consistent success responses
#[macro_export]
macro_rules! respond_success {
  ($context:expr, $title:expr, $description:expr) => {
    $context.reply_success($title, $description).await
  };
  ($context:expr, $description:expr) => {
    $context.reply_success("Success", $description).await
  };
}
