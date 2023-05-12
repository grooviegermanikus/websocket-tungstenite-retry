
use std::{str::FromStr, path::{Path, PathBuf}};
use std::future::Future;
use std::mem::size_of;
use std::ops::Deref;
use std::ptr::null;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;
use env_logger::Env;
use futures::future::join_all;
use futures::{future, SinkExt};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonrpsee::rpc_params;
use serde_json::{json, Value};
use serde_json::Value::Null;
use hyper::body::Bytes;
use log::info;
use serde::{Deserialize, Serialize};

use solana_client::rpc_client::{RpcClient, RpcClientConfig};
use solana_client::rpc_request::RpcRequest::Custom;
use solana_sdk::{pubkey::Pubkey, commitment_config::{CommitmentConfig, CommitmentLevel}, signature::Keypair, signer::Signer, transaction::Transaction};
use tokio::io::AsyncWriteExt;
use tokio::select;
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::sync::oneshot;
use tokio::time::Instant;
use tower::layer::util::{Identity, Stack};
use tower::ServiceBuilder;
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::LatencyUnit;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Span;
use url::Url;
use websocket_tungstenite_retry::websocket_stable::StableWebSocket;
use websocket_tungstenite_retry::websocket_stable::WsMessage::Text;


#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default()
        .default_filter_or("websocket_tungstenite_retry::websocket_stable=info")).init();

    let total_counter = AtomicI32::new(0);

    let mut list = Vec::new();
    for i in 0..10 {
        let service = createservice(format!("service-{}", i), total_counter.clone());
            list.push(service);
    }

    join_all(list).await;

}

async fn createservice(name: String, total_counter: Arc<AtomicBool>) {
    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse(
            "wss://api.mngo.cloud/orderbook/v1/"
        ).unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        }), Duration::from_secs(1)).unwrap();

    let mut count = 1;
    while let Some(Text(msg)) = ws.get_message_channel().recv().await {
        let mut value = serde_json::from_str::<Value>(msg.as_str()).unwrap();
        if value.get("update").is_some() {
            info!("{} - update#: {:?}", name, value.get("update").unwrap().as_array().unwrap().len());
        }


        if count % 10 == 0 {
            info!("{} - count: {}", name, count);
        }
        count += 1;


    }

    ws.join().await;

}
