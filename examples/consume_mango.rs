use env_logger::Env;
use serde_json::json;
use std::time::Duration;

use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug"),
    )
    .init();

    // let subscription_request = json!({
    //         "command": "subscribe",
    //         "marketId": market_id.to_string(),
    //     });
    //
    // let mut socket = StableWebSocket::new_with_timeout(
    //     Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
    //     subscription_request, Duration::from_secs(10)).await.unwrap();

    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        }),
        Duration::from_secs(3),
    )
    .await
    .unwrap();

    let mut channel = ws.subscribe_message_channel();
    while let Ok(msg) = channel.recv().await {
        println!("msg: {:?}", msg);
    }

    ws.join().await;
}
