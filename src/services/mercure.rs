use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::header::SET_COOKIE;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::error::SpoolerResult;
use crate::models::Config;
use crate::services::print_job::{JobContext, fetch_print_jobs};
use crate::services::websocket::print_created_job;
use crate::utils::config::read_config;
use crate::utils::http::with_auth_header;

const CHANNEL: &str = "private-print_job.";

/// The hub sends a heartbeat every 40 seconds by default.
const SILENCE_LIMIT: Duration = Duration::from_secs(90);

#[derive(serde::Deserialize)]
struct Authorization {
    channel_names: Vec<AuthorizedChannel>,
    expires_in: u64,
    topic_prefix: String,
}

#[derive(serde::Deserialize)]
struct AuthorizedChannel {
    #[serde(default)]
    denied: bool,
}

#[derive(serde::Deserialize)]
struct Update {
    event: String,
    payload: serde_json::Value,
}

pub async fn mercure_task(
    config: Arc<RwLock<Config>>,
    ctx: JobContext,
    cancel_token: CancellationToken,
) {
    if !read_config(&config).mercure_enabled {
        info!("Mercure is disabled. Not subscribing to the hub");
        return;
    }

    while !cancel_token.is_cancelled() {
        if let Err(e) = subscribe(&read_config(&config), &ctx, &cancel_token).await {
            error!(error = %e, "Mercure subscription failed, retrying in 5 seconds");
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = time::sleep(Duration::from_secs(5)) => {}
            }
        }
    }

    info!("Mercure task shutting down");
}

/// Listen until the subscriber token expires, then return so the caller
/// renews it. Events published in between are picked up by the job fetch at
/// the start of the next subscription.
async fn subscribe(
    config: &Config,
    ctx: &JobContext,
    cancel_token: &CancellationToken,
) -> SpoolerResult<()> {
    let response = with_auth_header(
        ctx.http_client
            .post(format!("{}/api/broadcasting/auth", config.flux_url)),
        config,
    )
    .header("Accept", "application/json")
    .json(&serde_json::json!({ "channel_names": [CHANNEL] }))
    .send()
    .await?;

    if !response.status().is_success() {
        return Err(format!("Mercure authorization failed: {}", response.status()).into());
    }

    let token = subscriber_token(
        response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok()),
    )
    .ok_or("Mercure authorization returned no subscriber token")?;

    let authorization: Authorization = response.json().await?;

    if authorization
        .channel_names
        .iter()
        .any(|channel| channel.denied)
    {
        return Err(format!("The API token may not subscribe {}", CHANNEL).into());
    }

    let hub = config
        .mercure_hub_url
        .clone()
        .unwrap_or_else(|| format!("{}/.well-known/mercure", config.flux_url));
    let topic = format!(
        "{}channel/{}",
        authorization.topic_prefix,
        urlencoding::encode(CHANNEL)
    );
    let lifetime = renew_after(authorization.expires_in);

    let mut stream = ctx
        .http_client
        .get(format!("{}?match={}", hub, urlencoding::encode(&topic)))
        .bearer_auth(token)
        .header("Accept", "text/event-stream")
        .timeout(lifetime + SILENCE_LIMIT)
        .send()
        .await?;

    if !stream.status().is_success() {
        return Err(format!("Mercure hub refused the subscription: {}", stream.status()).into());
    }

    info!(hub = %hub, "Subscribed to Mercure hub");

    let catch_up_config = config.clone();
    let catch_up_ctx = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = fetch_print_jobs(&catch_up_config, &catch_up_ctx).await {
            error!(error = %e, "Failed to fetch pending print jobs");
        }
    });

    let expiry = time::sleep(lifetime);
    tokio::pin!(expiry);
    let mut buffer = Vec::new();

    loop {
        let chunk = tokio::select! {
            _ = cancel_token.cancelled() => return Ok(()),
            _ = &mut expiry => return Ok(()),
            chunk = time::timeout(SILENCE_LIMIT, stream.chunk()) => {
                chunk.map_err(|_| "Mercure hub went silent")??
            }
        };

        let Some(chunk) = chunk else {
            return Err("Mercure hub closed the connection".into());
        };

        buffer.extend_from_slice(&chunk);

        for data in take_events(&mut buffer) {
            match serde_json::from_str::<Update>(&data) {
                Ok(update) => print_created_job(
                    &update.event,
                    &update.payload.to_string(),
                    config.clone(),
                    ctx.clone(),
                ),
                Err(e) => error!(error = %e, raw_data = %data, "Failed to parse Mercure update"),
            }
        }
    }
}

pub(crate) fn renew_after(expires_in: u64) -> Duration {
    Duration::from_secs(expires_in * 4 / 5)
}

pub(crate) fn subscriber_token<'a>(cookies: impl IntoIterator<Item = &'a str>) -> Option<String> {
    cookies.into_iter().find_map(|cookie| {
        let (name, rest) = cookie.split_once('=')?;
        name.to_ascii_lowercase()
            .contains("mercure")
            .then(|| rest.split(';').next().unwrap_or_default().to_owned())
    })
}

/// Remove every complete server-sent event from the buffer and return the
/// data of those that carry any.
pub(crate) fn take_events(buffer: &mut Vec<u8>) -> Vec<String> {
    let mut events = Vec::new();

    while let Some(end) = buffer.windows(2).position(|pair| pair == b"\n\n") {
        let block: Vec<u8> = buffer.drain(..end + 2).collect();
        let data = String::from_utf8_lossy(&block)
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|line| line.strip_prefix(' ').unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");

        if !data.is_empty() {
            events.push(data);
        }
    }

    events
}
