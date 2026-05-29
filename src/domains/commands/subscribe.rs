use poise::{
    serenity_prelude::{Colour, CreateEmbed},
    CreateReply,
};

use super::{is_valid_eth_address, short_addr, CommandContext, CommandError};

/// Subscribe to an orchestrator's notifications.
#[poise::command(slash_command, ephemeral)]
pub async fn subscribe(
    ctx: CommandContext<'_>,
    #[description = "Orchestrator address (0x...)"] orchestrator: String,
) -> Result<(), CommandError> {
    let user_id = ctx.author().id.to_string();
    let addr = orchestrator.trim().to_lowercase();

    if !is_valid_eth_address(&addr) {
        ctx.send(error_reply(
            "Invalid orchestrator address — expected `0x` followed by 40 hex characters.",
        ))
        .await?;
        return Ok(());
    }

    let data = ctx.data();
    let count = data.subscriptions.count_for_user(&user_id).await?;
    if count >= data.max_subscriptions_per_user as i64 {
        ctx.send(error_reply(&format!(
            "You're at the subscription cap ({}). Use `/unsubscribe` first.",
            data.max_subscriptions_per_user
        )))
        .await?;
        return Ok(());
    }

    let orch = match data.explorer.get_orchestrator(&addr).await {
        Ok(o) => o,
        Err(err) => {
            tracing::info!(?err, %addr, "subscribe: orchestrator lookup failed");
            ctx.send(error_reply(&format!(
                "Couldn't find an orchestrator at `{}`. Check the address and try again.",
                short_addr(&addr)
            )))
            .await?;
            return Ok(());
        }
    };

    let inserted = data.subscriptions.insert(&user_id, &addr).await?;
    let name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| short_addr(&addr));

    if inserted {
        // Seed delegator_history for this orchestrator so the first Bond
        // event after subscription is correctly classified as new vs. stake
        // change. Failures here are logged but don't block the subscription.
        if let Err(err) = crate::seed::seed_one(&data.explorer, &data.streams, &addr).await {
            tracing::warn!(?err, %addr, "subscribe: delegator_history seed failed");
        }
    }

    let title;
    let msg;
    if inserted {
        title = "Subscribed";
        msg = format!(
            "Now following **{}** (`{}`). You're subscribed to {} of {} orchestrators.\n\n\
             📩 Notifications arrive as DMs. Make sure we share a server and that **\"Allow direct messages from server members\"** is enabled, or I won't be able to reach you — `/subscriptions` shows delivery status.",
            name,
            short_addr(&addr),
            count + 1,
            data.max_subscriptions_per_user
        );
    } else {
        title = "Already subscribed";
        msg = format!(
            "You're already following **{}** (`{}`).",
            name,
            short_addr(&addr)
        );
    }

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title(title)
                .description(msg)
                .colour(Colour::from_rgb(0x46, 0xa7, 0x58)),
        ),
    )
    .await?;

    Ok(())
}

fn error_reply(msg: &str) -> CreateReply {
    CreateReply::default().ephemeral(true).embed(
        CreateEmbed::new()
            .title("Error")
            .description(msg)
            .colour(Colour::from_rgb(0xd0, 0x4a, 0x4a)),
    )
}
