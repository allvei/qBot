use serenity::all::{CreateEmbed as CE, CreateInteractionResponse as CIR, CreateInteractionResponseMessage as CIRM, UserId};
use crate::handlers::{build_settings_buttons, build_settings_embed};
use crate::handlers::settings::{PlayerSettings, ServerSettings, build_player_settings_buttons, build_player_settings_embed, build_server_settings_buttons, build_server_settings_embed};
use crate::repositories::UserSettings;

pub struct Ephemeral {
}

impl Ephemeral {
    pub fn send(embed: CE) -> CIR {
        CIR::Message(CIRM::new().embed(embed).ephemeral(true))
    }

    pub fn send_prefs(prefs: &UserSettings) -> CIR {
        let embed   = build_settings_embed(prefs);
        let buttons = build_settings_buttons(prefs);
        CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true))
    }

    pub fn send_config(settings: &ServerSettings, guild_name: &str) -> CIR {
        let embed   = build_server_settings_embed(settings, guild_name);
        let buttons = build_server_settings_buttons(settings, guild_name);
        CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true))
    }

    pub fn send_edit_player(settings: &PlayerSettings, user_id: UserId) -> CIR {
        let embed   = build_player_settings_embed(settings);
        let buttons = build_player_settings_buttons(user_id);
        CIR::Message(CIRM::new().embed(embed).components(buttons).ephemeral(true))
    }
}