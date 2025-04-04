use anyhow::bail;
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use url::Url;

use crate::websocket_stable::WebsocketHighLevelError::*;
use log::{debug, error, info, trace, warn};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};
use tokio::{select, sync};
use tokio_tungstenite::tungstenite::error::UrlError::UnableToConnect;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tokio_tungstenite::tungstenite::{error::Error as WsError, Error, Message};
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_util::{bytes::Bytes, sync::CancellationToken};

// power of 2
const CHANNEL_SIZE: usize = 65536;

// TOKIO-TUNGSTENITE

/*
   resilient websocket service based on tokio tungstenite

   the websocket service consist of a controller type and a worker thread

   * client interacts only with the controller
   * controller interacts with the worker thread via a channel
   * worker thread interacts with websocket server

*/

pub struct StableWebSocket {
    // channel to send payload messages from worker thread to client
    message_subscription: sync::broadcast::Sender<WsMessage>,
    status_receiver: UnboundedReceiver<StatusUpdate>,
    control_sender: UnboundedSender<ControlMessage>,
    state: State,
}

#[derive(Debug)]
enum State {
    Started(JoinHandle<()>),
    ShuttingDown,
    Stopped,
}

#[derive(Clone)]
struct ConnectionParams {
    url: Url,
    is_subscription_confirmed_message: fn(&str) -> anyhow::Result<()>,
}

impl StableWebSocket {
    pub async fn new(url: Url, subscription: Value) -> anyhow::Result<Self> {
        Self::new_with_timeout(url, subscription, Duration::from_millis(500)).await
    }

    /// url: e.g. wss://your.server.org/ws
    pub async fn new_with_timeout(
        url: Url,
        subscription: Value,
        startup_timeout: Duration,
    ) -> anyhow::Result<Self> {
        Self::new_with_timeout_and_success_callback(
            url,
            subscription,
            is_subscription_confirmed_message_solanarpc_mango,
            startup_timeout,
        )
        .await
    }

    pub async fn new_with_timeout_and_success_callback(
        url: Url,
        subscription: Value,
        success_callback: fn(&str) -> anyhow::Result<()>,
        startup_timeout: Duration,
    ) -> anyhow::Result<Self> {
        debug!(
            "WebSocket subscribe to url {:?} (timeout {:?})",
            url, startup_timeout
        );
        let (message_tx, _message_rx) = sync::broadcast::channel(CHANNEL_SIZE);
        let (sc_tx, mut sc_rx) = sync::mpsc::unbounded_channel();
        let (cc_tx, cc_rx) = sync::mpsc::unbounded_channel();

        // main thread
        let sender = message_tx.clone();
        let connection_params = ConnectionParams {
            url: url.clone(),
            is_subscription_confirmed_message: success_callback,
        };
        let join_handle = tokio::spawn(async move {
            debug!("WebSocket worker thread started");
            listen_and_handle_reconnects(
                connection_params,
                startup_timeout,
                sender,
                sc_tx,
                cc_rx,
                &subscription,
            )
            .await;
            debug!("WebSocket loop exhausted by close frame");

            // done - loop exhausted by close frame
        });

        let subscription_prototype = message_tx.clone();
        // blocking channel and wait for one Subscribed message
        if let Some(StatusUpdate::Subscribed) = sc_rx.recv().await {
            debug!("WebSocket subscribed successfully");
            Ok(Self {
                // need to call .subscribe
                message_subscription: subscription_prototype,
                status_receiver: sc_rx,
                control_sender: cc_tx,
                state: State::Started(join_handle),
            })
        } else {
            bail!("no subscription success - aborting")
        }
    }

    pub fn subscribe_message_channel(&mut self) -> Receiver<WsMessage> {
        self.message_subscription.subscribe()
    }

    pub async fn join(self) {
        match self.state {
            State::Started(join_handle) => {
                join_handle.await.unwrap();
            }
            State::ShuttingDown => {}
            State::Stopped => {}
        }
    }

    pub async fn shutdown(&mut self) {
        match &self.state {
            State::Started(_join_handle) => {
                self.state = State::ShuttingDown;
                // note: control_sender might get closed; subsequent sends will fail
                self.control_sender.send(ControlMessage::Shutdown).unwrap();
                debug!("shutting down websocket service");

                // wait for shutting down message
                loop {
                    let Some(status) = self.status_receiver.recv().await else {
                        break;
                    };
                    trace!("status: {:?}", status);

                    match status {
                        StatusUpdate::ShuttingDown => {
                            self.state = State::Stopped;
                            break;
                        }
                        _ => {
                            continue;
                        }
                    }
                }
            }
            State::ShuttingDown => {
                // shutdown in progress
                debug!("ignore duplicated shutdown request")
            }
            State::Stopped => {
                // already stopped - do nothing
            }
        }
    }
}

async fn listen_and_handle_reconnects<T: Serialize>(
    connection_params: ConnectionParams,
    startup_timeout: Duration,
    sender: sync::broadcast::Sender<WsMessage>,
    status_sender: UnboundedSender<StatusUpdate>,
    mut control_receiver: UnboundedReceiver<ControlMessage>,
    sub: &T,
) {
    let mut start_ts = Instant::now();

    let mut interval = interval(Duration::from_millis(800));

    while let Err(high_level_error) = connect_and_listen(
        connection_params.clone(),
        sender.clone(),
        &status_sender,
        &mut control_receiver,
        sub,
    )
    .await
    {
        match high_level_error {
            ConnectionWsError(e) => {
                error!("Can't connect - retry: {:?}", e);
                let elapsed_since_start = Instant::now() - start_ts;
                if elapsed_since_start > startup_timeout {
                    error!(
                        "ws error after timeout reached, time={:?} - aborting",
                        Instant::now() - start_ts
                    );
                    return;
                }
                interval.tick().await;
                start_ts = Instant::now();
                continue;
            }
            RecoverableWsError(e) => {
                error!("Recoverable wserror - retry: {:?}", e);
                interval.tick().await;
                continue;
            }
            RecoverableMiscError => {
                error!("Recoverable misc error - retry");
                interval.tick().await;
                continue;
            }
            FatalWsError(e) => {
                // TODO what should happen here?
                panic!("Fatal error: {:?}", e);
            }
            WebsocketHighLevelError::SubscriptionError => {
                warn!("Subscription error - aborting");
                return;
            }
        }
    } // -- loop over errors

    // ok result means "close frame received"
    debug!("Websocket connection closed after receiving websocket close frame from server");
}

/// payload messages from worker thread to client
#[derive(Debug, Clone)]
pub enum WsMessage {
    Text(Utf8Bytes),
    Binary(Bytes),
}

/// control messages from worker thread to client
#[derive(Debug, Clone)]
pub enum StatusUpdate {
    Subscribed,
    ShuttingDown,
    SubscriptionError,
}

/// control messages from client to worker thread
#[derive(Debug, Clone)]
pub enum ControlMessage {
    Shutdown,
}

const TIMEOUT_WARNING: Duration = Duration::from_millis(1500);

async fn connect_and_listen<T: Serialize>(
    connection_params: ConnectionParams,
    sender: sync::broadcast::Sender<WsMessage>,
    status_sender: &UnboundedSender<StatusUpdate>,
    control_receiver: &mut UnboundedReceiver<ControlMessage>,
    sub: &T,
) -> Result<(), WebsocketHighLevelError> {
    let url = connection_params.url;
    let is_subscription_confirmed_message = connection_params.is_subscription_confirmed_message;
    debug!("Connecting to websocket url <{}>", url.to_string());
    let (ws_stream, response) = connect_async(url).await.map_err(ConnectionWsError)?;
    assert_eq!(
        response.status(),
        101,
        "Error connecting to the server: {:?}",
        response
    );

    let shutdown = CancellationToken::new();

    let (mut ws_write, mut ws_read) = ws_stream.split();

    let json_value: Value = json!(sub);
    ws_write
        .send(tungstenite::Message::text(
            serde_json::to_string(&json_value).unwrap(),
        ))
        .await
        .map_err(map_error)?;

    // ping thread - not joined
    let shutdown_ping = shutdown.clone();
    tokio::spawn(async move {
        let mut interval_ping = interval(Duration::from_millis(1500));
        loop {
            select! {
                _ = shutdown_ping.cancelled() => {
                    ws_write.close().await.ok();
                    info!("Shutdown signal received - stopping ping thread");
                    return;
                }
                _ = interval_ping.tick() => {
                    ws_write.send(tungstenite::Message::Ping(Bytes::from(vec![13,37,42]))).await.ok();
                    debug!("Websocket Ping sent (period={:?})", interval_ping.period());
                }
            }
        }

        // ws_read.reunite(ws_write).unwrap().close(None).await.unwrap();
    });

    let shutdown_copy = shutdown.clone();
    let mut subscription_confirmed = false;
    loop {
        select! {
            _ = shutdown_copy.cancelled() => {
                debug!("shutting down");
                return Ok(());
            }
            maybe_msg = timeout(TIMEOUT_WARNING, ws_read.next()) => {
                match maybe_msg {
                    Ok(Some(msg)) => {
                        match msg.map_err(map_error)? {
                            tungstenite::Message::Text(s) => {
                                if !subscription_confirmed {
                                    match is_subscription_confirmed_message(s.as_str()) {
                                        Ok(_s) => {
                                            debug!("Subscription confirmed");
                                            subscription_confirmed = true;
                                            status_sender.send(StatusUpdate::Subscribed).expect("Can't send to channel");
                                            continue;
                                        }
                                        Err(err) => {
                                            info!("Subscription failed with <{}> - shutting down", err);
                                            return Err(WebsocketHighLevelError::SubscriptionError);
                                        }
                                    }
                                }
                                debug!("Received Text: {}", s);
                                if sender.receiver_count() > 0 {
                                    sender.send(WsMessage::Text(s)).expect("Can't send to channel");
                                } else {
                                    debug!("Dropping message - no receivers");
                                }
                            }
                            tungstenite::Message::Binary(data) => {
                                debug!("Received Binary: {} bytes", data.len());
                                if sender.receiver_count() > 0 {
                                    sender.send(WsMessage::Binary(data)).expect("Can't send to channel");
                                } else {
                                    debug!("Dropping message - no receivers");
                                }
                            }
                            // this close frame might be send from websocket server but in most cases
                            // it is the server's response to our close request triggered by Shutdown message
                            tungstenite::Message::Close(_) => {
                                debug!("Received websocket close frame");
                                return Ok(());
                            }
                            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                        }
                    }
                    Ok(None) => {
                        debug!("Subscription stream closed");
                        return Err(RecoverableMiscError);
                    }
                    Err(_elapsed) => {
                        debug!("Subscription timed out with no data - continue");
                    }
                }
            }
            Some(control_msg) = control_receiver.recv() => {
                match control_msg {
                    ControlMessage::Shutdown => {
                        shutdown.cancel();
                        info!("Signal shutdown");
                    }
                }
                // ok
            }
        }
    } // -- loop over messages
}

// TODO use trait /template pattern
pub fn is_subscription_confirmed_message_solanarpc_mango(s: &str) -> anyhow::Result<()> {
    // unsure if all servers return that information
    //  {"success":true,"message":"subscribed to level updates for Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1"}

    // Solana RPC returns "{\"jsonrpc\":\"2.0\",\"result\":7454,\"id\":1}"

    let maybe_value = serde_json::from_str::<Value>(s).unwrap();
    let mango_success = maybe_value["success"].as_bool().unwrap_or(false);
    let solanarpc_success = maybe_value["jsonrpc"]
        .as_str()
        .map(|s| s == "2.0")
        .unwrap_or(false)
        && !maybe_value["error"].is_object();

    if mango_success || solanarpc_success {
        debug!("Subscription success message: {:?}", s);
        Ok(())
    } else {
        warn!("Unexpected subscription response message: {:?}", s);
        bail!("Unexpected subscription response message: {:?}", s);
    }
}

#[allow(clippy::enum_variant_names)]
enum WebsocketHighLevelError {
    ConnectionWsError(WsError),
    // TODO add max retries
    RecoverableWsError(WsError),
    RecoverableMiscError,
    FatalWsError(WsError),
    SubscriptionError,
}

fn map_error(e: Error) -> WebsocketHighLevelError {
    match e {
        Error::ConnectionClosed => {
            error!("Connection closed: {}", e);
            FatalWsError(e)
        }
        Error::AlreadyClosed => RecoverableWsError(e),
        Error::Io(_) => RecoverableWsError(e),
        Error::Tls(_) => FatalWsError(e),
        Error::Capacity(_) => RecoverableWsError(e),
        Error::Protocol(_) => RecoverableWsError(e),
        Error::Utf8 => FatalWsError(e),
        Error::Url(UnableToConnect(_)) => RecoverableWsError(e),
        Error::Url(_) => FatalWsError(e),
        // e.g. Recoverable error - retry: Http(Response { status: 401, version: HTTP/1.1, ...)
        Error::Http(_) => RecoverableWsError(e),
        Error::HttpFormat(_) => RecoverableWsError(e),
        Error::WriteBufferFull(_) => RecoverableWsError(e),
        Error::AttackAttempt => FatalWsError(e),
    }
}
