use env_logger::Env;
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

    let mut ws = StableWebSocket::new(
        Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        }),
    )
    .await
    .unwrap();
    let mut count = 0;
    let mut msg_count = 0;

    while let Ok(msg) = ws.subscribe_message_channel().recv().await {
        println!("msg: {:?}", msg);
        msg_count += 1;
        count += 1;

        if count >= 5 {
            println!("shutting down");
            ws.shutdown().await;
            // should log "ignore duplicate shutdown request"
            ws.shutdown().await;
            // should log "ignore duplicate shutdown request"
            ws.shutdown().await;
        }

        assert!(msg_count <= 5, "msg_count: {}", msg_count);
    }

    ws.join().await;
}
