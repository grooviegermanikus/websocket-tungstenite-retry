use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::net::Shutdown::Read;
use std::net::TcpStream;
use std::ops::{Add, Deref};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use anyhow::{anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use url::Url;

use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{io, select, sync};
use tokio::sync::{Mutex, oneshot, RwLock};
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{Sender, unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{interval, sleep, sleep_until, timeout};
use tokio_tungstenite::{connect_async, tungstenite, WebSocketStream};
use tokio_tungstenite::tungstenite::{connect, WebSocket, error::Error as WsError, Error, Message};
use tokio_tungstenite::tungstenite::client::connect_with_config;
use tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake;
use tokio_tungstenite::tungstenite::error::UrlError::UnableToConnect;
use tokio_tungstenite::tungstenite::http::Response;
use tokio_tungstenite::tungstenite::stream::MaybeTlsStream;
use tokio_util::sync::CancellationToken;
use crate::websocket_stable::State::Started;
use crate::websocket_stable::WebsocketHighLevelError::{ConnectionWsError, FatalWsError, RecoverableWsError};

const CHANNEL_SIZE: usize = 1000;

// TOKIO-TUNGSTENITE

/*
    resilient websocket service based on tokio tungstenite

    the websocket service consist of a controller type and a worker thread

    * client interacts only with the controller
    * controller interacts with the worker thread via a channel
    * worker thread interacts with websocket server

 */

pub struct StableWebSocket {
    /// webserver url (e.g. wss://api.dydx.exchange/v3/ws)
    ws_url: Url,
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

impl StableWebSocket {

    pub async fn new(url: Url, subscription: Value) -> anyhow::Result<Self> {
        Self::new_with_timeout(url, subscription, Duration::from_millis(500)).await
    }
    /// url: e.g. wss://your.server.org/ws
    pub async fn new_with_timeout(url: Url, subscription: Value, startup_timeout: Duration) -> anyhow::Result<Self> {
        debug!("WebSocket subscribe to url {:?} (timeout {:?})", url, startup_timeout);
        let (message_tx, mut message_rx) = sync::broadcast::channel(CHANNEL_SIZE);
        let (sc_tx, mut sc_rx) = sync::mpsc::unbounded_channel();
        let (cc_tx, cc_rx) = sync::mpsc::unbounded_channel();

        // main thread
        let sender = message_tx.clone();
        let url2 = url.clone();
        let join_handle = tokio::spawn(async move {
            debug!("WebSocket worker thread started");
            listen_and_handle_reconnects(&url2, startup_timeout, sender, sc_tx, cc_rx, &subscription).await;
            debug!("WebSocket loop exhausted by close frame");
            return; // loop exhausted by close frame
        });

        let subscription_prototype = message_tx.clone();
        // blocking channel and wait for one Subscribed message
        if let Some(StatusUpdate::Subscribed) = sc_rx.recv().await {
            debug!("WebSocket subscribed successfully");
            Ok(Self {
                ws_url: url,
                // need to call .subscribe
                message_subscription: subscription_prototype,
                status_receiver: sc_rx,
                control_sender: cc_tx,
                state: State::Started(join_handle)
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
            State::ShuttingDown => {
            }
            State::Stopped => {
            }
        }
    }

    pub async fn shutdown(&mut self) {
        match &self.state {
            State::Started(join_handle) => {
                self.state = State::ShuttingDown;
                // note: control_sender might get closed; subsequent sends will fail
                self.control_sender.send(ControlMessage::Shutdown).unwrap();
                debug!("shutting down websocket service");

                // wait for shutting down message
                loop {
                    let Some(status) = self.status_receiver.recv().await else { break; };

                    println!("status: {:?}", status);

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

async fn listen_and_handle_reconnects<T: Serialize>(url: &Url,
                                                    startup_timeout: Duration,
                                                    sender: sync::broadcast::Sender<WsMessage>,
                                                    status_sender: UnboundedSender<StatusUpdate>,
                                                    mut control_receiver: UnboundedReceiver<ControlMessage>,
                                                    sub: &T) {
    let start_ts = Instant::now();

    let mut interval = interval(Duration::from_millis(200));

    while let Err(highlevel_error) = connect_and_listen(url, &sender, &status_sender, &mut control_receiver, sub).await {
        match highlevel_error {
            ConnectionWsError(e) => {
                error!("Can't connect - retry: {:?}", e);
                if start_ts.add(startup_timeout) < Instant::now() {
                    info!("abort on ws error after timeout reached: {:?}", Instant::now() - start_ts);
                    return;
                }
                interval.tick().await;
                continue;
            }
            RecoverableWsError(e) => {
                error!("Recoverable error - retry: {:?}", e);
                interval.tick().await;
                continue;
            }
            FatalWsError(e) => {
                // TODO what should happen here?
                panic!("Fatal error: {:?}", e);
            }
        }
    } // -- loop over errors

    // ok result means "close frame received"
    debug!("Websocket connection closed after receiving websocket close frame from server");
}

/// payload messages from worker thread to client
#[derive(Debug, Clone)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
}

/// control messages from worker thread to client
#[derive(Debug, Clone)]
pub enum StatusUpdate {
    Subscribed,
    ShuttingDown,
}

/// control messages from client to worker thread
#[derive(Debug, Clone)]
pub enum ControlMessage {
    Shutdown,
}

async fn connect_and_listen<T: Serialize>(
    url: &Url, sender: &sync::broadcast::Sender<WsMessage>, status_sender: &UnboundedSender<StatusUpdate>,
    control_receiver: &mut UnboundedReceiver<ControlMessage>,
    sub: &T) -> Result<(), WebsocketHighLevelError> {
    let (mut ws_stream, response) = connect_async(url).await.map_err(|e| ConnectionWsError(e))?;
    assert_eq!(response.status(), 101, "Error connecting to the server: {:?}", response);

    let shutdown = CancellationToken::new();

    let (mut ws_write, mut ws_read) = ws_stream.split();

    let json_value: Value = json!(sub);
    ws_write.send( tungstenite::Message::text(serde_json::to_string(&json_value).unwrap())).await
        .map_err(|e| map_error(e))?;

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
                    ws_write.send(tungstenite::Message::Ping(vec![13,37,42])).await.ok();
                    debug!("Websocket Ping sent (period={:?})", interval_ping.period());
                }
            }
        }


        // ws_read.reunite(ws_write).unwrap().close(None).await.unwrap();
    });

    let shutdown_copy = shutdown.clone();
    let interval = interval(Duration::from_millis(1500));
    let mut subscription_confirmed = false;
    loop {
        select! {
            _ = shutdown_copy.cancelled() => {
                debug!("shutting down");
                return Ok(());
            }
            Some(msg) = ws_read.next() => {
                 match msg.map_err(|e| map_error(e))? {
                    tungstenite::Message::Text(s) => {
                        if !subscription_confirmed && is_subscription_confirmed_message(s.as_str()) {
                            debug!("Subscription confirmed");
                            subscription_confirmed = true;
                            status_sender.send(StatusUpdate::Subscribed).expect("Can't send to channel");
                            continue;
                        }
                        debug!("Received Text: {}", s);
                        sender.send(WsMessage::Text(s.clone())).expect("Can't send to channel");
                    }
                    tungstenite::Message::Binary(data) => {
                        debug!("Received Binary: {} bytes", data.len());
                        sender.send(WsMessage::Binary(data)).expect("Can't send to channel");
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

    Ok(())
}

// TODO use trait /template pattern
fn is_subscription_confirmed_message(s: &str) -> bool {
    // unsure if all servers return that information
    //  {"success":true,"message":"subscribed to level updates for Fgh9JSZ2qfSjCw9RPJ85W2xbihsp2muLvfRztzoVR7f1"}

    let maybe_value = serde_json::from_str::<Value>(&s).unwrap();
    let success = maybe_value["success"].as_bool().unwrap();
    if success {
        debug!("Subscription success message: {:?}", s);
    } else {
        warn!("Unexpected subscription response message: {:?}", s);
    }

    success
}

enum WebsocketHighLevelError {
    ConnectionWsError(WsError),
    // TODO add max retries
    RecoverableWsError(WsError),
    FatalWsError(WsError),
}

fn map_error(e: Error) -> WebsocketHighLevelError {
    match e {
        Error::ConnectionClosed => {
            error!("Connection closed: {}", e);
            return FatalWsError(e);
        }
        Error::AlreadyClosed => RecoverableWsError(e),
        Error::Io(_) => RecoverableWsError(e),
        Error::Tls(_) => FatalWsError(e),
        Error::Capacity(_) => RecoverableWsError(e),
        Error::Protocol(_) => RecoverableWsError(e),
        Error::SendQueueFull(_) => FatalWsError(e),
        Error::Utf8 => FatalWsError(e),
        Error::Url(UnableToConnect(_)) => RecoverableWsError(e),
        Error::Url(_) => FatalWsError(e),
        // e.g. Recoverable error - retry: Http(Response { status: 401, version: HTTP/1.1, ...)
        Error::Http(_) => RecoverableWsError(e),
        Error::HttpFormat(_) => RecoverableWsError(e),
    }
}
