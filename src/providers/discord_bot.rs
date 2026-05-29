//! Bot-authenticated Discord HTTP client for sending direct messages.
//!
//! Wraps `serenity::http::Http` so we get its built-in bucket-aware rate
//! limiting for free. Exposes a single async method `send_dm` plus a typed
//! error so callers can distinguish "user closed DMs" from other failures
//! and drive the auto-unsubscribe logic in 004b's reward poller.

use std::sync::Arc;

use poise::serenity_prelude::{self as serenity, CreateMessage, UserId};

#[derive(Debug, thiserror::Error)]
pub enum DmError {
    /// 403 from Discord — the recipient has DMs disabled, has blocked the
    /// bot, or no longer shares a guild with it. `code` is the Discord JSON
    /// error code when present (50007 = "Cannot send messages to this user",
    /// the privacy/no-mutual-guild case) so callers can log the distinction.
    #[error("recipient has DMs closed or blocked the bot (discord code {code:?})")]
    DmsClosed { code: Option<i64> },
    /// 429 after serenity exhausted its internal retries.
    #[error("rate limited")]
    RateLimited,
    /// Anything else: network, 5xx, malformed payload, etc.
    #[error("send failed: {0}")]
    Other(String),
}

pub struct BotDmSender {
    http: Arc<serenity::Http>,
}

impl BotDmSender {
    pub fn new(bot_token: &str) -> Self {
        Self {
            http: Arc::new(serenity::Http::new(bot_token)),
        }
    }

    pub async fn send_dm(&self, user_id: u64, message: CreateMessage) -> Result<(), DmError> {
        let user = UserId::new(user_id);
        let channel = user
            .create_dm_channel(&*self.http)
            .await
            .map_err(classify_error)?;
        channel
            .id
            .send_message(&*self.http, message)
            .await
            .map_err(classify_error)?;
        Ok(())
    }
}

fn classify_error(err: serenity::Error) -> DmError {
    if let serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(resp)) = &err {
        return match resp.status_code.as_u16() {
            403 => DmError::DmsClosed {
                code: Some(resp.error.code as i64),
            },
            429 => DmError::RateLimited,
            _ => DmError::Other(err.to_string()),
        };
    }
    DmError::Other(err.to_string())
}
