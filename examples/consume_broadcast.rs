use env_logger::Env;
use futures_util::future::join_all;
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

    {
        // try create+drop
        let mut channel = ws.subscribe_message_channel();
        let _ = channel.recv().await;
    }

    let mut channel_a = ws.subscribe_message_channel();
    let join_handle_a = tokio::spawn(async move {
        let mut count = 0;
        while let Ok(msg) = channel_a.recv().await {
            println!("msgA: {:?}", msg);
            count += 1;

            if count > 5 {
                return;
            }
        }
    });

    let mut channel_b = ws.subscribe_message_channel();
    let join_handle_b = tokio::spawn(async move {
        let mut count = 0;
        while let Ok(msg) = channel_b.recv().await {
            println!("msgB: {:?}", msg);
            count += 1;

            if count > 10 {
                return;
            }
        }
    });

    join_all(vec![join_handle_a, join_handle_b]).await;

    ws.shutdown().await;

    ws.join().await;
}
