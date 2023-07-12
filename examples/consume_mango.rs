use std::{str::FromStr, path::{Path, PathBuf}};
use std::future::Future;
use std::mem::size_of;
use std::ptr::null;
use std::time::Duration;
use env_logger::Env;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::rpc_params;
use serde_json::json;
use serde_json::Value::Null;

use tokio::sync::mpsc::{Sender, UnboundedSender};
use tracing::Span;
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
    while let Some(msg) = ws.get_message_channel().recv().await {
        println!("msg: {:?}", msg);

        if count > 5 {
            println!("shutting down");
            ws.shutdown();
        }
        count += 1;
    }

    ws.join().await;

}
