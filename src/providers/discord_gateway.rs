//! Discord gateway runtime — owns the `poise` `Framework` + `serenity`
//! `Client`. Spawned as a 4th tokio task by `runtime::run` when
//! `COMMANDS_ENABLED=true`. Drives until shutdown signal.
//!
//! Command registration:
//!   - If `DISCORD_GUILD_ID` is set, commands are registered to that guild
//!     only (instantaneous updates — use during development).
//!   - Otherwise commands are registered globally (1-hour propagation delay
//!     per Discord docs).

use std::sync::Arc;

use poise::serenity_prelude::{self as serenity, GatewayIntents, GuildId};

use crate::{
    config::CommandsConfig,
    domains::{
        commands::{all_commands, BotData, CommandError},
        explorer::client::ExplorerClient,
        subscriptions::repo::SqliteSubscriptionsRepo,
    },
};

pub async fn run(
    config: CommandsConfig,
    explorer: Arc<ExplorerClient>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
) -> anyhow::Result<()> {
    let max_subs = config.max_subscriptions_per_user;
    let bot_token = config.bot_token.clone();
    let guild_id = config.guild_id;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: all_commands(),
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(move |ctx, _ready, framework| {
            Box::pin(async move {
                match guild_id {
                    Some(id) => {
                        poise::builtins::register_in_guild(
                            ctx,
                            &framework.options().commands,
                            GuildId::new(id),
                        )
                        .await?;
                        tracing::info!(guild_id = id, "registered commands in guild");
                    }
                    None => {
                        poise::builtins::register_globally(ctx, &framework.options().commands)
                            .await?;
                        tracing::info!("registered commands globally");
                    }
                }
                Ok(BotData {
                    explorer,
                    subscriptions,
                    max_subscriptions_per_user: max_subs,
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(&bot_token, GatewayIntents::non_privileged())
        .framework(framework)
        .await?;

    let shard_manager = client.shard_manager.clone();

    tokio::spawn(async move {
        if let Err(err) = client.start().await {
            tracing::error!(?err, "discord gateway client exited");
        }
    });

    // Park: serenity drives forever; we just wait for ctrl_c. `runtime::run`
    // handles the overall shutdown but if anything triggers it we want the
    // gateway to disconnect cleanly.
    let _ = tokio::signal::ctrl_c().await;
    shard_manager.shutdown_all().await;
    Ok(())
}

async fn on_error(error: poise::FrameworkError<'_, BotData, CommandError>) {
    match error {
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!(
                error = %error,
                command = ctx.command().qualified_name,
                "command handler failed"
            );
            let _ = ctx
                .send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content(format!("Something went wrong: {error}")),
                )
                .await;
        }
        other => {
            tracing::error!(?other, "poise framework error");
        }
    }
}
