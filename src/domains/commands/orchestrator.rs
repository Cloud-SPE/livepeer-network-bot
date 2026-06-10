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

    let description = format_delegators_description(&addr, &resp.data, total);

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

fn format_delegators_description(
    orch_addr: &str,
    delegators: &[crate::domains::explorer::types::OrchDelegatorRow],
    total_lpt: f64,
) -> String {
    if delegators.is_empty() {
        return format!("No delegators found for `{}`.", short_addr(orch_addr));
    }

    let mut buf = String::new();
    for (i, d) in delegators.iter().enumerate() {
        let bonded_lpt = parse_f64_or_zero(&d.bonded_principal);
        let pct = if total_lpt > 0.0 {
            100.0 * bonded_lpt / total_lpt
        } else {
            0.0
        };
        let _ = writeln!(
            buf,
            "**#{}** `{}` — {:.2} LPT ({:.2}%)",
            i + 1,
            short_addr(&d.delegator_address),
            bonded_lpt,
            pct
        );
    }
    let _ = write!(
        buf,
        "\n_Top {} by stake; total shown: {:.2} LPT_",
        delegators.len(),
        total_lpt
    );
    buf
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
        Some(r) => format_rewards_description(r, period.label(), from, to),
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

/// The explorer rewards leaderboard returns token sums already denominated
/// in whole LPT (e.g. "6180.605443…"), NOT wei — do not rescale. Verified
/// against onchain BondingManager `Reward` event sums.
fn format_rewards_description(
    r: &crate::domains::explorer::types::RewardLeaderboardRow,
    period_label: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> String {
    let total_lpt = parse_f64_or_zero(&r.sum_total_tokens);
    let orch_lpt = parse_f64_or_zero(&r.sum_orch_tokens);
    let delegators_lpt = parse_f64_or_zero(&r.sum_delegators_tokens);
    let total_usd = parse_f64_or_zero(&r.sum_total_tokens_usd);
    let orch_usd = parse_f64_or_zero(&r.sum_orch_tokens_usd);
    let count = parse_f64_or_zero(&r.reward_event_count) as u64;
    format!(
        "**{}** ({} – {})\n\n\
         Reward events: **{}**\n\
         Total distributed: **{:.4} LPT** (${:.2})\n\
         Orchestrator cut: **{:.4} LPT** (${:.2})\n\
         Delegators cut: **{:.4} LPT**",
        period_label, from, to, count, total_lpt, total_usd, orch_lpt, orch_usd, delegators_lpt,
    )
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

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{format_delegators_description, format_rewards_description};
    use crate::domains::explorer::types::{OrchDelegatorRow, RewardLeaderboardRow};

    fn delegator(addr: &str, bonded_principal: &str) -> OrchDelegatorRow {
        OrchDelegatorRow {
            delegator_address: addr.to_string(),
            bonded_principal: bonded_principal.to_string(),
            pending_stake: None,
            pending_fees: None,
            pending_round: None,
            as_of_block: "0".into(),
            as_of_timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn delegators_description_uses_lpt_units_directly() {
        let rows = vec![
            delegator(
                "0xde42f514869714f911fb61f9a07f6149fcb3c52c",
                "7269.467093857653",
            ),
            delegator(
                "0x8db248cba18678df52b1093b675385da94587dfe",
                "3628.178738145608",
            ),
        ];
        let total = 10897.645832003261;

        let description = format_delegators_description(
            "0xb120a72a9264e90092e8197c0fabd210c18bc5be",
            &rows,
            total,
        );

        assert!(description.contains("**#1** `0xde42…c52c` — 7269.47 LPT (66.71%)"));
        assert!(description.contains("**#2** `0x8db2…7dfe` — 3628.18 LPT (33.29%)"));
        assert!(description.contains("_Top 2 by stake; total shown: 10897.65 LPT_"));
    }

    /// Values are the live API response for lpt.moudi.eth, week 2026-06-01 –
    /// 2026-06-07, cross-checked against the sum of the 8 onchain
    /// BondingManager `Reward` events for that window (6180.605443589182 LPT).
    #[test]
    fn rewards_description_uses_lpt_units_directly() {
        let row = RewardLeaderboardRow {
            orchestrator_address: "0x141e6d4953b933746c770272126db2bd691a9683".into(),
            display_name: Some("lpt.moudi.eth".into()),
            avatar_url: None,
            reward_event_count: "8".into(),
            sum_total_tokens: "6180.605443589182597266".into(),
            sum_total_tokens_usd: "11704.171079767097644062".into(),
            sum_orch_tokens: "185.418163307675477918".into(),
            sum_orch_tokens_usd: "351.125132393012929321".into(),
            sum_delegators_tokens: "5995.187280281507119348".into(),
            sum_delegators_tokens_usd: "11353.045947374084714741".into(),
            usd_rows_priced: "8".into(),
        };
        let from = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 6, 7).unwrap();

        let description = format_rewards_description(&row, "Weekly", from, to);

        assert!(description.contains("Reward events: **8**"));
        assert!(description.contains("Total distributed: **6180.6054 LPT** ($11704.17)"));
        assert!(description.contains("Orchestrator cut: **185.4182 LPT** ($351.13)"));
        assert!(description.contains("Delegators cut: **5995.1873 LPT**"));
    }
}
