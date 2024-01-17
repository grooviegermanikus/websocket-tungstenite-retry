use std::env;
use std::thread::Thread;
use env_logger::Env;
use serde_json::json;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::time::sleep;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug"),
    )
        .init();

    let rpc_url = "wss://api.mngo.cloud/lite-rpc/v1/".to_string();

    let mut ws = loop {
        let attempt = StableWebSocket::new_with_timeout(
            Url::parse(rpc_url.as_str()).unwrap(),
            // https://solana.com/docs/rpc/websocket/slotsubscribe#code-sample
            json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "slotSubscribe",
        }), Duration::from_secs(5)
        ).await;

        match &attempt {
            Ok(connected) => {
                println!("connected!");
                break attempt.unwrap();
            }
            Err(_) => {
                println!("retrying from scratch...");
                sleep(Duration::from_millis(300)).await;
                continue;
            }
        }
    };

    println!("connected - continue");

    let mut channel = ws.subscribe_message_channel();
    let mut count = 0;
    while let Ok(msg) = channel.recv().await {
        println!("msg: {:?}", msg);
        count += 1;
    }

    ws.join().await;
}
