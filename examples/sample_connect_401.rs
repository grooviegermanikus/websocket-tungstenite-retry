use env_logger::Env;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::future::Future;
use std::time::Duration;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug"),
    )
    .init();

    let result = StableWebSocket::new_with_timeout(
        Url::parse("ws://httpbin.org/status/401").unwrap(),
        json!({
            "command": "subscribe",
        }),
        Duration::from_secs(3),
    )
    .await;

    assert_eq!(result.is_err(), true);
}
