

use std::future::Future;
use std::time::Duration;
use env_logger::Env;
use serde_json::json;
use serde::{Deserialize, Serialize};
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default()
        .default_filter_or("websocket_tungstenite_retry::websocket_stable=debug")).init();

    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse(
            "ws://localhost:2020/"
        ).unwrap(),
        json!({
            "command": "subscribe",
        }), Duration::from_secs(1)).await.unwrap();

    while let Some(msg) = ws.subscribe_message_channel().recv().await {
        println!("msg: {:?}", msg);
    }

    ws.join().await;

}

