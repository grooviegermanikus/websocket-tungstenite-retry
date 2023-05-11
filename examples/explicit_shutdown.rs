
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
use hyper::body::Bytes;
use serde::{Deserialize, Serialize};

use solana_client::rpc_client::{RpcClient, RpcClientConfig};
use solana_client::rpc_request::RpcRequest::Custom;
use solana_sdk::{pubkey::Pubkey, commitment_config::{CommitmentConfig, CommitmentLevel}, signature::Keypair, signer::Signer, transaction::Transaction, info};
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tower::layer::util::{Identity, Stack};
use tower::ServiceBuilder;
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::LatencyUnit;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
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
        })).unwrap();
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

