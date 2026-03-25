use crate::handlers::settings::{build_server_settings_buttons, build_server_settings_embed, ServerSettings};
use crate::handlers::{build_settings_buttons, build_settings_embed};
use crate::repo::UserPreferences;
use serenity::all::{ButtonStyle as BS, CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};

pub struct Ephemeral {}

impl Ephemeral {
  pub fn send(embed: CE) -> CIR {
    CIR::Message(CIRM::new().embed(embed).ephemeral(true))
  }

  pub fn send_prefs(prefs: &UserPreferences) -> CIR {
    let embed = build_settings_embed(prefs);
    let buttons = build_settings_buttons(prefs);
    CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true))
  }

  pub fn send_config(settings: &ServerSettings, guild_name: &str) -> CIR {
    let embed = build_server_settings_embed(settings, guild_name);
    let buttons = build_server_settings_buttons(settings, guild_name);
    CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true))
  }

  pub fn update(embed: CE) -> CIR {
    CIR::UpdateMessage(CIRM::new().embed(embed).components(vec![]))
  }

  pub fn update_with(embed: CE, components: Vec<serenity::all::CreateActionRow>) -> CIR {
    CIR::UpdateMessage(CIRM::new().embed(embed).components(components))
  }

  pub fn back(id: impl Into<String>) -> CB {
    CB::new(id.into()).label("Back").style(BS::Secondary)
  }
}
