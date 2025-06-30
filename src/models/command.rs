// CHECK ME
use std::sync::Arc;
use serenity::all::{CommandInteraction, Context};
use crate::database::Database;

#[derive(Clone)]
pub struct CommandContext<'a> {
    pub ctx:   &'a Context,
    pub intax: &'a CommandInteraction,
    pub db:    Arc<Database>,
}
