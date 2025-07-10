use serenity::all::{CreateEmbedFooter, Embed};
use serenity::builder::CreateEmbed;

pub async fn create_embed(
    title: &str,
    description: &str,
    footer: &str,
) -> CreateEmbed {
    CreateEmbed::new().title(title).description(description).footer(CreateEmbedFooter::new(footer))
}

pub async fn update_dashboard(_dash: Embed) {
    // Buttons:
    // - Toggle join/leave
    // - Shuffle
    // - Start
    // - End
}
