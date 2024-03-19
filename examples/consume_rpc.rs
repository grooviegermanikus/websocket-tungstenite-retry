use std::env;
use env_logger::Env;
use serde_json::json;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedSender};
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug"),
    )
    .init();

    let rpc_url = format!("wss://api.testnet.rpcpool.com/{TESTNET_API_TOKEN}",
                          TESTNET_API_TOKEN = std::env::var("TESTNET_API_TOKEN").unwrap());

    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse(rpc_url.as_str()).unwrap(),
        // https://solana.com/docs/rpc/websocket/slotsubscribe#code-sample
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "slotSubscribe",
        }),
        Duration::from_secs(3),
    )
    .await
    .unwrap();

    let mut channel = ws.subscribe_message_channel();
    let mut count = 0;
    while let Ok(msg) = channel.recv().await {
        println!("msg: {:?}", msg);
        count += 1;
    }

    ws.join().await;
}
