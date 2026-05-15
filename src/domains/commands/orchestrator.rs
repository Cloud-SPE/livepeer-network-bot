//! `/orchestrator <subcommand>` group.
//!
//! Three subcommands all keyed by a 0x address:
//!
//!   /orchestrator delegators <address>
//!   /orchestrator rewards    <address> <period>
//!   /orchestrator tickets    <address> <period>
//!
//! All replies are ephemeral. Period is `daily | weekly | monthly` and always
//! refers to the LAST complete UTC period, never today-so-far.

use std::fmt::Write;

use chrono::{Datelike, NaiveDate, Utc};
use poise::{
    serenity_prelude::{Colour, CreateEmbed},
    CreateReply,
};

use super::{is_valid_eth_address, parse_f64_or_zero, short_addr, CommandContext, CommandError};

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum PeriodChoice {
    #[name = "daily"]
    Daily,
    #[name = "weekly"]
    Weekly,
    #[name = "monthly"]
    Monthly,
}

impl PeriodChoice {
    fn label(&self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }
}

/// Parent command — never invoked directly; subcommands fan out below it.
#[poise::command(
    slash_command,
    subcommands("delegators", "rewards", "tickets"),
    subcommand_required
)]
pub async fn orchestrator(_: CommandContext<'_>) -> Result<(), CommandError> {
    Ok(())
}

#[poise::command(slash_command, ephemeral)]
pub async fn delegators(
    ctx: CommandContext<'_>,
    #[description = "Orchestrator address (0x...)"] orchestrator: String,
) -> Result<(), CommandError> {
    let addr = orchestrator.trim().to_lowercase();
    if !is_valid_eth_address(&addr) {
        return ctx
            .send(error_reply("Invalid orchestrator address."))
            .await
            .map(|_| ())
            .map_err(Into::into);
    }

    let resp = ctx
        .data()
        .explorer
        .orchestrator_delegators(&addr, None, 10)
        .await?;

    let total: f64 = resp
        .data
        .iter()
        .map(|d| parse_f64_or_zero(&d.bonded_principal))
        .sum();

    let description = if resp.data.is_empty() {
        format!("No delegators found for `{}`.", short_addr(&addr))
    } else {
        let mut buf = String::new();
        for (i, d) in resp.data.iter().enumerate() {
            let bonded = parse_f64_or_zero(&d.bonded_principal) / 1e18;
            let total_lpt = total / 1e18;
            let pct = if total_lpt > 0.0 {
                100.0 * bonded / total_lpt
            } else {
                0.0
            };
            let _ = writeln!(
                buf,
                "**#{}** `{}` — {:.2} LPT ({:.2}%)",
                i + 1,
                short_addr(&d.delegator_address),
                bonded,
                pct
            );
        }
        let _ = write!(
            buf,
            "\n_Top {} by stake; total shown: {:.2} LPT_",
            resp.data.len(),
            total / 1e18
        );
        buf
    };

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title(format!("Delegators of {}", short_addr(&addr)))
                .description(description)
                .colour(Colour::from_rgb(0x46, 0xa7, 0x58)),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, ephemeral)]
pub async fn rewards(
    ctx: CommandContext<'_>,
    #[description = "Orchestrator address (0x...)"] orchestrator: String,
    #[description = "Time period"] period: PeriodChoice,
) -> Result<(), CommandError> {
    let addr = orchestrator.trim().to_lowercase();
    if !is_valid_eth_address(&addr) {
        ctx.send(error_reply("Invalid orchestrator address."))
            .await?;
        return Ok(());
    }

    let (from, to) = period_window(period, Utc::now().date_naive());

    let resp = ctx
        .data()
        .explorer
        .rewards_leaderboard(from, to, 200)
        .await?;

    let row = resp
        .data
        .iter()
        .find(|r| r.orchestrator_address.to_lowercase() == addr);

    let description = match row {
        Some(r) => {
            let total_lpt = parse_f64_or_zero(&r.sum_total_tokens) / 1e18;
            let orch_lpt = parse_f64_or_zero(&r.sum_orch_tokens) / 1e18;
            let delegators_lpt = parse_f64_or_zero(&r.sum_delegators_tokens) / 1e18;
            let total_usd = parse_f64_or_zero(&r.sum_total_tokens_usd);
            let orch_usd = parse_f64_or_zero(&r.sum_orch_tokens_usd);
            let count = parse_f64_or_zero(&r.reward_event_count) as u64;
            format!(
                "**{}** ({} – {})\n\n\
                 Reward events: **{}**\n\
                 Total distributed: **{:.4} LPT** (${:.2})\n\
                 Orchestrator cut: **{:.4} LPT** (${:.2})\n\
                 Delegators cut: **{:.4} LPT**",
                period.label(),
                from,
                to,
                count,
                total_lpt,
                total_usd,
                orch_lpt,
                orch_usd,
                delegators_lpt,
            )
        }
        None => format!(
            "No reward activity for `{}` in {} ({} – {}).",
            short_addr(&addr),
            period.label().to_lowercase(),
            from,
            to
        ),
    };

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title(format!("Rewards · {}", short_addr(&addr)))
                .description(description)
                .colour(Colour::from_rgb(0xff, 0xa5, 0x00)),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, ephemeral)]
pub async fn tickets(
    ctx: CommandContext<'_>,
    #[description = "Orchestrator address (0x...)"] orchestrator: String,
    #[description = "Time period"] period: PeriodChoice,
) -> Result<(), CommandError> {
    let addr = orchestrator.trim().to_lowercase();
    if !is_valid_eth_address(&addr) {
        ctx.send(error_reply("Invalid orchestrator address."))
            .await?;
        return Ok(());
    }

    let (from, to) = period_window(period, Utc::now().date_naive());

    let resp = ctx
        .data()
        .explorer
        .payout_leaderboard(from, to, 200)
        .await?;

    let row = resp
        .data
        .iter()
        .find(|r| r.orchestrator_address.to_lowercase() == addr);

    let description = match row {
        Some(r) => {
            let face_eth = parse_f64_or_zero(&r.sum_face_value_native);
            let face_usd = parse_f64_or_zero(&r.sum_face_value_usd);
            let commission_eth = parse_f64_or_zero(&r.sum_commission_native);
            let commission_usd = parse_f64_or_zero(&r.sum_commission_usd);
            let delegators_eth = parse_f64_or_zero(&r.sum_delegators_share_native);
            let count = parse_f64_or_zero(&r.ticket_count) as u64;
            let gateways = parse_f64_or_zero(&r.distinct_gateways) as u64;
            format!(
                "**{}** ({} – {})\n\n\
                 Tickets: **{}** from {} gateway(s)\n\
                 Total face value: **{:.4} ETH** (${:.2})\n\
                 Commission: **{:.4} ETH** (${:.2})\n\
                 Delegators' share: **{:.4} ETH**",
                period.label(),
                from,
                to,
                count,
                gateways,
                face_eth,
                face_usd,
                commission_eth,
                commission_usd,
                delegators_eth,
            )
        }
        None => format!(
            "No ticket activity for `{}` in {} ({} – {}).",
            short_addr(&addr),
            period.label().to_lowercase(),
            from,
            to
        ),
    };

    ctx.send(
        CreateReply::default().ephemeral(true).embed(
            CreateEmbed::new()
                .title(format!("Tickets · {}", short_addr(&addr)))
                .description(description)
                .colour(Colour::from_rgb(0xff, 0xd7, 0x00)),
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

/// For the given period, returns the `[from, to]` date range for the LAST
/// complete UTC period. Daily = yesterday; Weekly = last Mon–Sun; Monthly =
/// previous month start–end.
fn period_window(period: PeriodChoice, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match period {
        PeriodChoice::Daily => {
            let d = today - chrono::Duration::days(1);
            (d, d)
        }
        PeriodChoice::Weekly => {
            let this_monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let last_monday = this_monday - chrono::Duration::days(7);
            let last_sunday = last_monday + chrono::Duration::days(6);
            (last_monday, last_sunday)
        }
        PeriodChoice::Monthly => {
            let first_of_this =
                NaiveDate::from_ymd_opt(today.year(), today.month(), 1).expect("valid");
            let last_day_prev = first_of_this - chrono::Duration::days(1);
            let first_of_prev =
                NaiveDate::from_ymd_opt(last_day_prev.year(), last_day_prev.month(), 1)
                    .expect("valid");
            (first_of_prev, last_day_prev)
        }
    }
}
