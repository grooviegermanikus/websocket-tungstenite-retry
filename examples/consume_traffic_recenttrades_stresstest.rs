use env_logger::Env;
use serde_json::{json, Value};
use std::env;
use std::thread::Thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use anyhow::bail;

use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::time::sleep;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::{StableWebSocket, WsMessage};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=info"),
    )
    .init();

    let websocket_url = env::var("WEBSOCKET_URL").expect("WEBSOCKET_URL must be set");

    for i in 1..=500 {
        tokio::spawn(start_client(websocket_url.clone(), format!("client-{}", i)));
    }

    sleep(Duration::from_secs(1800)).await;
    println!("DONE - shutting down");
}

async fn start_client(ws_url: String, client_id: String) {

    let mut ws = loop {
        let attempt = StableWebSocket::new_with_timeout_and_success_callback(
            Url::parse(ws_url.as_str()).unwrap(),
            json!({
                "command": "subscribe",
                "mintIds": ["So11111111111111111111111111111111111111112-So11111111111111111111111111111111111111112"],
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
        let epoch_ms_last_digits = (unix_millis_now() % 100_000) as f64;
        println!("[{}, epoch_digits {}] msg: {:?}", client_id, epoch_ms_last_digits, msg);

        if let WsMessage::Text(msg) = msg {
            let value = serde_json::from_str::<Value>(&msg).unwrap();
            println!("msg = {}", value);
        }

        count += 1;
    }

    ws.join().await;
}


fn unix_millis_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis()
}

