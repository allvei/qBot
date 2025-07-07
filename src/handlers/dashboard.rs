use serenity::{all::Embed, builder::CreateEmbed};

pub async fn create_embed(title: &str, description: Option<&str>, footer: Option<&str>) -> Self {
    let embed = CreateEmbed::new()
        .title(title)
        .description(description)
        .footer(CreateEmbedFooter::new(footer));
    embed
}

pub async fn update_dashboard(dash: Embed) {
    // Buttons:
    // - Toggle join/leave
    // - Shuffle
    // - Start
    // - End
}