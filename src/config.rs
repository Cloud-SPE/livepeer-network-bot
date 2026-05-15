use std::time::Duration;

use url::Url;

#[derive(Debug, Clone)]
pub struct Config {
    pub explorer_base_url: Url,
    pub discord_webhook_url: Url,
    pub database_url: String,
    pub event_poll_interval: Duration,
    pub digest_window: Duration,
    pub digest_fetch_limit: u32,
    pub summary_poll_interval: Duration,
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
        let discord_webhook_url = url_var("DISCORD_WEBHOOK_URL")?;
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?;

        let event_poll_interval = secs_var("EVENT_POLL_INTERVAL_SECS", 60)?;
        let digest_window = secs_var("DIGEST_WINDOW_SECS", 15 * 60)?;
        let digest_fetch_limit = u32_var("DIGEST_FETCH_LIMIT", 500)?;
        let summary_poll_interval = secs_var("SUMMARY_POLL_INTERVAL_SECS", 60 * 60)?;
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
            discord_webhook_url,
            database_url,
            event_poll_interval,
            digest_window,
            digest_fetch_limit,
            summary_poll_interval,
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
