use std::fmt::Write;

use poise::{
    serenity_prelude::{Colour, CreateEmbed},
    CreateReply,
};

use super::{short_addr, CommandContext, CommandError};

#[poise::command(slash_command, ephemeral)]
pub async fn subscriptions(ctx: CommandContext<'_>) -> Result<(), CommandError> {
    let user_id = ctx.author().id.to_string();
    let data = ctx.data();
    let subs = data.subscriptions.list_for_user(&user_id).await?;

    let description = if subs.is_empty() {
        "You're not subscribed to any orchestrators yet. Use `/subscribe <address>` to start."
            .to_string()
    } else {
        let mut buf = format!(
            "You follow {} of {} orchestrators:\n\n",
            subs.len(),
            data.max_subscriptions_per_user
        );
        for s in &subs {
            // Look up display name lazily so the list still renders if any
            // single lookup fails.
            let name = match data
                .explorer
                .get_orchestrator(&s.orchestrator_address)
                .await
            {
                Ok(o) => o
                    .display_name
                    .unwrap_or_else(|| s.orchestrator_address.clone()),
                Err(_) => s.orchestrator_address.clone(),
            };
            let flag = if s.dm_blocked {
                "  ⚠️ DM-blocked"
            } else {
                ""
            };
            let _ = writeln!(
                buf,
                "• **{}** — `{}`{}",
                name,
                short_addr(&s.orchestrator_address),
                flag
            );
        }
        if subs.iter().any(|s| s.dm_blocked) {
            buf.push_str(
                "\n⚠️ Entries marked **DM-blocked** mean I couldn't deliver a DM to you, so notifications are paused (your subscription is kept). Enable **\"Allow direct messages from server members\"** in Discord and make sure we share a server — delivery resumes automatically on the next successful DM.",
            );
        }
        buf
    };

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title("Your subscriptions")
                .description(description)
                .colour(Colour::from_rgb(0x46, 0xa7, 0x58)),
        ),
    )
    .await?;
    Ok(())
}
