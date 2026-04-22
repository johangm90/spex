use std::time::Duration;

use chrono::Utc;
use serde_json::json;

use crate::config::WebhookConfig;

/// Fire a webhook for the given event type and payload.
///
/// - If `config` is `None` → no-op
/// - If `config.events` is non-empty and `event_type` is not in the list → no-op
/// - On any HTTP error (network, timeout, non-2xx) → print warning to stderr, return
/// - Never propagates errors (fail-graceful)
pub async fn fire(config: Option<&WebhookConfig>, event_type: &str, payload: serde_json::Value) {
    let config = match config {
        Some(c) => c,
        None => return,
    };

    // If an explicit event filter is set, skip events not in the list.
    if !config.events.is_empty() && !config.events.iter().any(|e| e == event_type) {
        return;
    }

    let body = json!({
        "event": event_type,
        "timestamp": Utc::now().to_rfc3339(),
        "payload": payload,
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("⚠ webhook failed: {}", e);
            return;
        }
    };

    match client.post(&config.url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            eprintln!("⚠ webhook failed: HTTP {}", resp.status());
        }
        Err(e) => {
            eprintln!("⚠ webhook failed: {}", e);
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WebhookConfig;

    fn make_config(events: Vec<&str>) -> WebhookConfig {
        WebhookConfig {
            url: "http://localhost:19999/webhook".to_string(),
            events: events.into_iter().map(|s| s.to_string()).collect(),
            timeout_secs: 2,
        }
    }

    #[tokio::test]
    async fn no_op_when_config_is_none() {
        // Should complete without panic or error.
        fire(None, "TaskDone", serde_json::json!({})).await;
    }

    #[tokio::test]
    async fn no_op_when_event_not_in_filter() {
        let cfg = make_config(vec!["SpecApproved"]);
        // "TaskDone" is not in the filter — should return immediately without
        // attempting a network call (no server running on port 19999).
        fire(Some(&cfg), "TaskDone", serde_json::json!({})).await;
    }

    #[tokio::test]
    async fn fires_when_event_matches_filter() {
        let cfg = make_config(vec!["TaskDone"]);
        // Network call will fail (no server), but the function must not panic.
        fire(Some(&cfg), "TaskDone", serde_json::json!({"id": "T001"})).await;
    }

    #[tokio::test]
    async fn fires_when_event_list_is_empty() {
        let cfg = make_config(vec![]);
        // Empty events list means "all events" — network call will fail gracefully.
        fire(Some(&cfg), "AnyEvent", serde_json::json!({})).await;
    }
}
