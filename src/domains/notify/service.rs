use serde_json::Value;

/// Port for delivering an embed payload to Discord (or any other future
/// channel). The bot's concrete implementation today is
/// `providers::discord::DiscordWebhook`.
///
/// The explicit `impl Future + Send` return is intentional and clippy's
/// `manual_async_fn` lint is silenced because `async fn` in a trait does not
/// yet imply a `Send` bound on the returned future; scheduler tasks require
/// `Send` to be spawned on the multi-thread runtime.
#[allow(clippy::manual_async_fn)]
pub trait Notifier: Send + Sync + 'static {
    fn send(&self, payload: Value) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}
