use std::{path::Path, time::Duration};

use url::Url;

#[derive(Debug, Clone)]
pub struct Config {
    pub explorer_base_url: Url,
    /// One or more Discord webhook URLs to which every public-channel embed
    /// is POSTed. Parsed from a comma-separated `DISCORD_WEBHOOK_URL`, so a
    /// single-server deploy sets one URL and a multi-server deploy lists
    /// several (one webhook per server channel). Posting fans out to all of
    /// them; see `providers::discord::FanOutNotifier`.
    pub discord_webhook_urls: Vec<Url>,
    pub database_url: String,
    pub event_poll_interval: Duration,
    pub digest_window: Duration,
    pub digest_fetch_limit: u32,
    /// When `false`, nothing is sent to any `discord_webhook_urls`: neither the
    /// per-orchestrator ticket digests nor the daily/weekly/monthly
    /// summaries are spawned. Used in dev to share a webhook URL with prod
    /// without double-posting. Events still poll and persist.
    pub webhook_post_enabled: bool,
    pub summary_poll_interval: Duration,
    pub summary_readiness: SummaryReadiness,
    pub reward_poll_interval: Duration,
    pub delegator_poll_interval: Duration,
    pub subscriber_digest_interval: Duration,
    pub http_timeout: Duration,
    pub user_agent: String,
    pub commands: Option<CommandsConfig>,
}

/// Configuration for the slash-command runtime. `None` means commands are
/// disabled and the bot runs in webhook-only mode (the v0 deploy shape).
#[derive(Debug, Clone)]
pub struct CommandsConfig {
    pub bot_token: String,
    pub application_id: u64,
    /// When `Some`, commands are registered per-guild (instant updates,
    /// useful during development). When `None`, registration is global.
    pub guild_id: Option<u64>,
    pub max_subscriptions_per_user: u32,
    /// Consecutive DM 403 failures before a subscription is auto-removed.
    pub dm_failure_auto_unsub: i64,
}

/// Gating thresholds that keep the daily/weekly/monthly summary poster from
/// publishing a rollup before the explorer has finished indexing, enriching,
/// and deriving the period's data. A period is never posted before
/// `period_end + settle_<cadence>`, and only once the rollup is fully priced,
/// not behind the bot's own ingested event count, and stable across two
/// consecutive polls. `max_defer` is the backstop: past `period_end +
/// max_defer` the rollup is posted with an "incomplete" marker rather than
/// being skipped silently. See `domains::scheduler::summary_poster`.
#[derive(Debug, Clone)]
pub struct SummaryReadiness {
    pub settle_daily: Duration,
    pub settle_weekly: Duration,
    pub settle_monthly: Duration,
    pub max_defer: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var: {0}")]
    Missing(&'static str),
    #[error("invalid value for {var}: {source}")]
    Invalid {
        var: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let explorer_base_url = url_var("EXPLORER_BASE_URL")?;
        let discord_webhook_urls = url_list_var("DISCORD_WEBHOOK_URL")?;
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        validate_database_url(&database_url, running_in_container())?;

        let event_poll_interval = secs_var("EVENT_POLL_INTERVAL_SECS", 60)?;
        let digest_window = secs_var("DIGEST_WINDOW_SECS", 15 * 60)?;
        let digest_fetch_limit = u32_var("DIGEST_FETCH_LIMIT", 500)?;
        let webhook_post_enabled = bool_var("WEBHOOK_POST_ENABLED", true)?;
        let summary_poll_interval = secs_var("SUMMARY_POLL_INTERVAL_SECS", 60 * 60)?;
        let summary_readiness = SummaryReadiness {
            settle_daily: secs_var("SUMMARY_SETTLE_DAILY_SECS", 6 * 60 * 60)?,
            settle_weekly: secs_var("SUMMARY_SETTLE_WEEKLY_SECS", 12 * 60 * 60)?,
            settle_monthly: secs_var("SUMMARY_SETTLE_MONTHLY_SECS", 24 * 60 * 60)?,
            max_defer: secs_var("SUMMARY_MAX_DEFER_SECS", 48 * 60 * 60)?,
        };
        let reward_poll_interval = secs_var("REWARD_POLL_INTERVAL_SECS", 60)?;
        let delegator_poll_interval = secs_var("DELEGATOR_POLL_INTERVAL_SECS", 60)?;
        let subscriber_digest_interval = secs_var("SUBSCRIBER_DIGEST_INTERVAL_SECS", 15 * 60)?;
        let http_timeout = secs_var("HTTP_TIMEOUT_SECS", 30)?;

        let user_agent =
            std::env::var("USER_AGENT").unwrap_or_else(|_| "livepeer-payout-bot/0.1".into());

        let commands = if bool_var("COMMANDS_ENABLED", false)? {
            Some(CommandsConfig::from_env()?)
        } else {
            None
        };

        Ok(Self {
            explorer_base_url,
            discord_webhook_urls,
            database_url,
            event_poll_interval,
            digest_window,
            digest_fetch_limit,
            webhook_post_enabled,
            summary_poll_interval,
            summary_readiness,
            reward_poll_interval,
            delegator_poll_interval,
            subscriber_digest_interval,
            http_timeout,
            user_agent,
            commands,
        })
    }
}

impl CommandsConfig {
    fn from_env() -> Result<Self, ConfigError> {
        let bot_token = std::env::var("DISCORD_BOT_TOKEN")
            .map_err(|_| ConfigError::Missing("DISCORD_BOT_TOKEN"))?;
        let application_id = u64_var("DISCORD_APPLICATION_ID")?;
        let guild_id = optional_u64_var("DISCORD_GUILD_ID")?;
        let max_subscriptions_per_user = u32_var("MAX_SUBSCRIPTIONS_PER_USER", 25)?;
        let dm_failure_auto_unsub = i64_var("DM_FAILURE_AUTO_UNSUB", 3)?;

        Ok(Self {
            bot_token,
            application_id,
            guild_id,
            max_subscriptions_per_user,
            dm_failure_auto_unsub,
        })
    }
}

fn url_var(name: &'static str) -> Result<Url, ConfigError> {
    let raw = std::env::var(name).map_err(|_| ConfigError::Missing(name))?;
    Url::parse(&raw).map_err(|e| ConfigError::Invalid {
        var: name,
        source: anyhow::Error::new(e),
    })
}

/// Parse a comma-separated list of URLs from one env var. Whitespace around
/// each entry is trimmed and empty entries (e.g. a trailing comma) are
/// skipped, so a single-URL value parses to a one-element list unchanged.
/// At least one valid URL is required.
fn url_list_var(name: &'static str) -> Result<Vec<Url>, ConfigError> {
    let raw = std::env::var(name).map_err(|_| ConfigError::Missing(name))?;
    parse_url_list(name, &raw)
}

fn parse_url_list(name: &'static str, raw: &str) -> Result<Vec<Url>, ConfigError> {
    let mut urls = Vec::new();
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let url = Url::parse(trimmed).map_err(|e| ConfigError::Invalid {
            var: name,
            source: anyhow::Error::new(e),
        })?;
        urls.push(url);
    }
    if urls.is_empty() {
        return Err(ConfigError::Missing(name));
    }
    Ok(urls)
}

fn secs_var(name: &'static str, default: u64) -> Result<Duration, ConfigError> {
    let raw = match std::env::var(name) {
        Ok(v) => v,
        Err(_) => return Ok(Duration::from_secs(default)),
    };
    let n = raw.parse::<u64>().map_err(|e| ConfigError::Invalid {
        var: name,
        source: anyhow::Error::new(e),
    })?;
    Ok(Duration::from_secs(n))
}

fn bool_var(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    let raw = match std::env::var(name) {
        Ok(v) => v,
        Err(_) => return Ok(default),
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" | "" => Ok(false),
        other => Err(ConfigError::Invalid {
            var: name,
            source: anyhow::anyhow!("expected boolean, got `{other}`"),
        }),
    }
}

fn u64_var(name: &'static str) -> Result<u64, ConfigError> {
    let raw = std::env::var(name).map_err(|_| ConfigError::Missing(name))?;
    raw.parse()
        .map_err(|e: std::num::ParseIntError| ConfigError::Invalid {
            var: name,
            source: anyhow::Error::new(e),
        })
}

fn optional_u64_var(name: &'static str) -> Result<Option<u64>, ConfigError> {
    let Ok(raw) = std::env::var(name) else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    raw.parse()
        .map(Some)
        .map_err(|e: std::num::ParseIntError| ConfigError::Invalid {
            var: name,
            source: anyhow::Error::new(e),
        })
}

fn u32_var(name: &'static str, default: u32) -> Result<u32, ConfigError> {
    let raw = match std::env::var(name) {
        Ok(v) => v,
        Err(_) => return Ok(default),
    };
    raw.parse()
        .map_err(|e: std::num::ParseIntError| ConfigError::Invalid {
            var: name,
            source: anyhow::Error::new(e),
        })
}

fn i64_var(name: &'static str, default: i64) -> Result<i64, ConfigError> {
    let raw = match std::env::var(name) {
        Ok(v) => v,
        Err(_) => return Ok(default),
    };
    raw.parse()
        .map_err(|e: std::num::ParseIntError| ConfigError::Invalid {
            var: name,
            source: anyhow::Error::new(e),
        })
}

fn validate_database_url(database_url: &str, in_container: bool) -> Result<(), ConfigError> {
    if !in_container || database_url == "sqlite::memory:" {
        return Ok(());
    }

    let Some(path) = database_url.strip_prefix("sqlite://") else {
        return Ok(());
    };

    if path.starts_with('/') {
        return Ok(());
    }

    Err(ConfigError::Invalid {
        var: "DATABASE_URL",
        source: anyhow::anyhow!(
            "containerized deploys must use an absolute SQLite path such as `sqlite:///data/livepeer-payout-bot.db`; relative paths like `{database_url}` are ephemeral inside the container"
        ),
    })
}

fn running_in_container() -> bool {
    Path::new("/.dockerenv").exists() || std::env::var_os("container").is_some()
}

#[cfg(test)]
mod tests {
    use super::{parse_url_list, validate_database_url};

    #[test]
    fn parses_single_webhook_url() {
        let urls = parse_url_list("DISCORD_WEBHOOK_URL", "https://discord.com/api/webhooks/a")
            .expect("single URL parses");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].as_str(), "https://discord.com/api/webhooks/a");
    }

    #[test]
    fn parses_comma_separated_webhook_urls_trimming_and_skipping_blanks() {
        let urls = parse_url_list(
            "DISCORD_WEBHOOK_URL",
            " https://discord.com/api/webhooks/a , https://discord.com/api/webhooks/b ,",
        )
        .expect("multiple URLs parse");
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].as_str(), "https://discord.com/api/webhooks/a");
        assert_eq!(urls[1].as_str(), "https://discord.com/api/webhooks/b");
    }

    #[test]
    fn empty_or_blank_webhook_list_is_missing() {
        assert!(parse_url_list("DISCORD_WEBHOOK_URL", "").is_err());
        assert!(parse_url_list("DISCORD_WEBHOOK_URL", "  , ,").is_err());
    }

    #[test]
    fn invalid_webhook_url_is_rejected() {
        assert!(parse_url_list("DISCORD_WEBHOOK_URL", "not a url").is_err());
        assert!(parse_url_list(
            "DISCORD_WEBHOOK_URL",
            "https://discord.com/api/webhooks/a, not-a-url"
        )
        .is_err());
    }

    #[test]
    fn allows_relative_sqlite_path_outside_container() {
        assert!(validate_database_url("sqlite://./livepeer-payout-bot.db", false).is_ok());
    }

    #[test]
    fn rejects_relative_sqlite_path_inside_container() {
        let err = validate_database_url("sqlite://./livepeer-payout-bot.db", true)
            .expect_err("relative SQLite path should be rejected in containers");
        assert_eq!(err.to_string(), "invalid value for DATABASE_URL: containerized deploys must use an absolute SQLite path such as `sqlite:///data/livepeer-payout-bot.db`; relative paths like `sqlite://./livepeer-payout-bot.db` are ephemeral inside the container");
    }

    #[test]
    fn allows_absolute_sqlite_path_inside_container() {
        assert!(validate_database_url("sqlite:///data/livepeer-payout-bot.db", true).is_ok());
    }
}
