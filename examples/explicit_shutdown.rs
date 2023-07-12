
use std::future::Future;
use std::time::Duration;
use env_logger::Env;
use serde_json::json;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;


#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug")).init();


    let mut ws = StableWebSocket::new(
        Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        })).await.unwrap();
    let mut count = 0;
    let mut msg_count = 0;

    while let Some(msg) = ws.get_message_channel().recv().await {
        println!("msg: {:?}", msg);
        msg_count += 1;
        count += 1;

        if count >= 10 {
            println!("shutting down");
            // note: might intentiently call shutdown multiple times
            ws.shutdown();
        }

        assert!(msg_count <= 10, "msg_count: {}", msg_count);
    }

    ws.join().await;

}

