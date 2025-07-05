// CHECK ME
use std::sync::Arc;

use serenity::all::{
    CommandInteraction, Context, CreateInteractionResponse, CreateInteractionResponseMessage,
};

use crate::database::Database;

#[derive(Clone)]
pub struct CommandContext<'a> {
    pub ctx:   &'a Context,
    pub intax: &'a CommandInteraction,
    pub db:    Arc<Database>,
}

impl CommandContext<'_> {
    pub async fn create_bot_reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .content(message)
                .ephemeral(true),
        );
        self.intax.create_response(&self.ctx.http, response).await?;
        Ok(())
    }
}
