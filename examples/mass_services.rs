use env_logger::Env;
use futures_util::future::join_all;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::atomic::AtomicI32;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;
use websocket_tungstenite_retry::websocket_stable::WsMessage::Text;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=info"),
    )
    .init();

    let total_counter = Arc::new(AtomicI32::new(0));

    let mut list = Vec::new();
    for i in 0..10 {
        let service = createservice(format!("service-{}", i), total_counter.clone());
        list.push(service);
    }

    join_all(list).await;
}

async fn createservice(name: String, total_counter: Arc<AtomicI32>) {
    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        }),
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    let mut count = 1;
    while let Ok(Text(msg)) = ws.subscribe_message_channel().recv().await {
        let value = serde_json::from_str::<Value>(msg.as_str()).unwrap();
        if value.get("update").is_some() {
            info!(
                "{} - update#: {:?}",
                name,
                value.get("update").unwrap().as_array().unwrap().len()
            );
        }

        if count % 10 == 0 {
            info!("{} - count: {}", name, count);
        }
        count += 1;
    }

    ws.join().await;
}
