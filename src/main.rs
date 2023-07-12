mod websocket_stable;

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

use tokio::sync::mpsc::{Sender, UnboundedSender};
use tracing::Span;
use url::Url;
use crate::websocket_stable::{StableWebSocket, WsMessage};


#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("websocket_tungstenite_retry::websocket_stable=debug")).init();

    let mut ws = StableWebSocket::new_with_timeout(
        Url::parse(
            // "wss://api.mngo.cloud/orderbook/v1/"
            "wss://api222.foooo.cloud/ordefdsafrbook/v1/"
        ).unwrap(),
        json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        }), Duration::from_secs(3)).unwrap();
    let mut count = 0;
    while let Some(msg) = ws.get_message_channel().recv().await {
        println!("msg: {:?}", msg);

        if count > 10 {
            println!("shutting down");
            ws.shutdown();
        }
        count += 1;
    }

    ws.join().await;

}

mod test {
    use std::sync::atomic::AtomicI32;
    use env_logger::Env;
    use log::info;
    use serde_json::json;
    use tracing_test::traced_test;
    use url::Url;
    use crate::websocket_stable::StableWebSocket;


    #[tokio::test(flavor = "multi_thread")]
    #[traced_test]
    async fn logo() {

        info!("Loogig");

    }

    #[tokio::test(flavor = "multi_thread")]
    async fn handle_401() {
        env_logger::init();

        // will block inside
        let mut ws = StableWebSocket::new(
            Url::parse("ws://httpbin.org/status/401").unwrap(),
            json!({
            "command": "subscribe",
        }));

    }

    // TODO add connection error test

    #[tokio::test(flavor = "multi_thread")]
    async fn consume_feed_then_shutdown() {
        let mut ws = StableWebSocket::new(
            Url::parse("wss://api.mngo.cloud/orderbook/v1/").unwrap(),
            json!({
            "command": "subscribe",
            "marketId": "Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1",
        })).unwrap();
        let mut count = 0;
        while let Some(msg) = ws.get_message_channel().recv().await {
            println!("msg: {:?}", msg);

            if count > 10 {
                println!("shutting down");
                ws.shutdown();
            }
            count += 1;
        }

        ws.join().await;

    }
}
