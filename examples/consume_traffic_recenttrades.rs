use env_logger::Env;
use serde_json::{json, Value};
use std::env;
use std::thread::Thread;
use std::time::Duration;
use anyhow::bail;

use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::time::sleep;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=info"),
    )
    .init();

    let ws_url = "wss://api.mngo.cloud/dojo/v1/".to_string();

    let mut ws = loop {
        let attempt = StableWebSocket::new_with_timeout_and_success_callback(
            Url::parse(ws_url.as_str()).unwrap(),
                json!({
                "command": "subscribe",
                "mintIds": ["J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn-So11111111111111111111111111111111111111112"],
            }),
            |msg| {
                // {"Status":{"success":true,"message":"subscribed to J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn-So11111111111111111111111111111111111111112 market with id: J1toso1uCk3RLmjorhTtrVwY9HJ7X8V9yYac6Y7kGCPn-So11111111111111111111111111111111111111112"}}
                let maybe_value = serde_json::from_str::<Value>(msg)?;
                match maybe_value["Status"]["success"].as_bool() {
                    Some(_) => Ok(()),
                    None => bail!("unexpected message: {}", msg)
                }
            },
            Duration::from_secs(5),
        ).await;

        match &attempt {
            Ok(connected) => {
                break attempt.unwrap();
            }
            Err(_) => {
                println!("retrying from scratch...");
                sleep(Duration::from_millis(300)).await;
                continue;
            }
        }
    };

    let mut channel = ws.subscribe_message_channel();
    let mut count = 0;
    while let Ok(msg) = channel.recv().await {
        println!("msg: {:?}", msg);
        count += 1;
    }

    ws.join().await;
}
