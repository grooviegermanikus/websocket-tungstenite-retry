


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
use serde::{Deserialize, Serialize};

use solana_client::rpc_client::{RpcClient, RpcClientConfig};
use solana_client::rpc_request::RpcRequest::Custom;
use solana_sdk::{pubkey::Pubkey, commitment_config::{CommitmentConfig, CommitmentLevel}, signature::Keypair, signer::Signer, transaction::Transaction, info};
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::time::Instant;
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
    env_logger::Builder::from_env(Env::default()
        .default_filter_or("websocket_tungstenite_retry::websocket_stable=debug")).init();

    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse(
            "ws://localhost:2020/"
        ).unwrap(),
        json!({
            "command": "subscribe",
        }), Duration::from_secs(1)).unwrap();

    while let Some(msg) = ws.get_message_channel().recv().await {
        println!("msg: {:?}", msg);
    }

    ws.join().await;

}

