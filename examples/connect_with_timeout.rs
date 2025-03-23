use env_logger::Env;
use serde_json::json;
use std::time::{Duration, Instant};
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug"),
    )
    .init();

    wait_1sec().await;
    wait_3secs().await;
}

async fn wait_1sec() {
    let start = Instant::now();
    let result = StableWebSocket::new_with_timeout(
        Url::parse("ws://notexists.mango.com/ws").unwrap(),
        json!({
            "command": "subscribe",
        }),
        Duration::from_secs(1),
    )
    .await;

    assert!(result.is_err());
    assert!(start.elapsed().as_secs() >= 1);
    assert!(start.elapsed().as_secs() < 2);
}

async fn wait_3secs() {
    let start = Instant::now();
    let result = StableWebSocket::new_with_timeout(
        Url::parse("ws://notexists.mango.com/ws").unwrap(),
        json!({
            "command": "subscribe",
        }),
        Duration::from_secs(3),
    )
    .await;

    assert!(result.is_err());
    assert!(start.elapsed().as_secs() >= 3);
    assert!(start.elapsed().as_secs() < 4);
}
