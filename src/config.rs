use std::time::Duration;

use url::Url;

#[derive(Debug, Clone)]
pub struct Config {
    pub explorer_base_url: Url,
    pub discord_webhook_url: Url,
    pub database_url: String,
    pub event_poll_interval: Duration,
    pub digest_window: Duration,
    pub summary_poll_interval: Duration,
    pub http_timeout: Duration,
    pub user_agent: String,
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
        let summary_poll_interval = secs_var("SUMMARY_POLL_INTERVAL_SECS", 60 * 60)?;
        let http_timeout = secs_var("HTTP_TIMEOUT_SECS", 30)?;

        let user_agent =
            std::env::var("USER_AGENT").unwrap_or_else(|_| "livepeer-payout-bot/0.1".into());

        Ok(Self {
            explorer_base_url,
            discord_webhook_url,
            database_url,
            event_poll_interval,
            digest_window,
            summary_poll_interval,
            http_timeout,
            user_agent,
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
