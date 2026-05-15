use reqwest::Client;

use crate::config::Config;

pub fn build(config: &Config) -> anyhow::Result<Client> {
    let client = Client::builder()
        .user_agent(&config.user_agent)
        .timeout(config.http_timeout)
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()?;
    Ok(client)
}
