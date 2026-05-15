use poise::{
    serenity_prelude::{Colour, CreateEmbed},
    CreateReply,
};

use super::{is_valid_eth_address, short_addr, CommandContext, CommandError};

#[poise::command(slash_command, ephemeral)]
pub async fn unsubscribe(
    ctx: CommandContext<'_>,
    #[description = "Orchestrator address (0x...)"] orchestrator: String,
) -> Result<(), CommandError> {
    let user_id = ctx.author().id.to_string();
    let addr = orchestrator.trim().to_lowercase();

    if !is_valid_eth_address(&addr) {
        ctx.send(
            CreateReply::default().ephemeral(true).embed(
                CreateEmbed::new()
                    .title("Error")
                    .description(
                        "Invalid orchestrator address — expected `0x` followed by 40 hex characters.",
                    )
                    .colour(Colour::from_rgb(0xd0, 0x4a, 0x4a)),
            ),
        )
        .await?;
        return Ok(());
    }

    let removed = ctx.data().subscriptions.delete(&user_id, &addr).await?;
    let (title, msg, colour) = if removed {
        (
            "Unsubscribed",
            format!("Stopped following `{}`.", short_addr(&addr)),
            Colour::from_rgb(0x46, 0xa7, 0x58),
        )
    } else {
        (
            "Not subscribed",
            format!("You weren't following `{}`.", short_addr(&addr)),
            Colour::from_rgb(0x96, 0x96, 0x96),
        )
    };

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title(title)
                .description(msg)
                .colour(colour),
        ),
    )
    .await?;
    Ok(())
}
