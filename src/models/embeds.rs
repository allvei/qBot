use crate::handlers::settings::{build_guild_config_buttons, build_guild_config_embed, ServerSettings};
use crate::handlers::settings::user_prefs_system::{get_user_prefs_menu_system, UserPrefsPage};
use crate::repo::UserPreferences;
use serenity::all::{ButtonStyle as BS, CreateButton as CB, CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM};

pub struct Ephemeral {}

impl Ephemeral {
  pub fn send(embed: CE) -> CIR {
    CIR::Message(CIRM::new().embed(embed).ephemeral(true))
  }

  pub fn send_prefs(prefs: &UserPreferences) -> CIR {
    let system = get_user_prefs_menu_system();
    if let Some(response) = system.build_response(UserPrefsPage::Main, prefs) {
      response
    } else {
      // Fallback to empty response if build fails
      CIR::Message(CIRM::new().content("Failed to build settings menu").ephemeral(true))
    }
  }

  pub fn send_config(settings: &ServerSettings, guild_name: &str) -> CIR {
    let embed = build_guild_config_embed(settings, guild_name);
    let buttons = build_guild_config_buttons(settings, guild_name);
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
