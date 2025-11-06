// CHECK ME
use std::sync::Arc;
use tokio::sync::Mutex;

use serenity::all::{
    CommandInteraction,
    ComponentInteraction,
    Context,
    CreateInteractionResponse as CIR,
    CreateInteractionResponseMessage as CIRM,
};
use crate::database::Database;
use crate::models::manager::Manager as GameManager;

#[derive(Clone)]
pub struct CommandContext<'a> {
    pub ctx:     &'a Context,
    pub intax:   &'a CommandInteraction,
    pub db:      Arc<Database>,
    pub manager: &'a Arc<Mutex<GameManager>>,
}

#[derive(Clone)]
pub struct ComponentContext<'a> {
    pub ctx:       &'a Context,
    pub component: &'a ComponentInteraction,
    pub db:        Arc<Database>,
}

impl CommandContext<'_> {
    pub async fn create_bot_reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(
            CIRM::new().content(message).ephemeral(true)
        );
        self.intax.create_response(&self.ctx.http, response).await?;
        Ok(())
    }
}

impl ComponentContext<'_> {
    pub async fn create_bot_reply(&self, message: &str) -> Result<(), anyhow::Error> {
        let response = CIR::Message(
            CIRM::new().content(message).ephemeral(true)
        );
        self.component.create_response(&self.ctx.http, response).await?;
        Ok(())
    }
}
