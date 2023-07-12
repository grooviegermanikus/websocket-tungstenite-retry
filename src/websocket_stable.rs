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
use futures::{SinkExt, StreamExt, TryStreamExt};
use futures::channel::mpsc::unbounded;
use futures::executor::block_on;
use url::Url;

use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{io, select, sync};
use tokio::sync::{Mutex, oneshot, RwLock};
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
use crate::websocket_stable::State::Started;
use crate::websocket_stable::WebsocketHighLevelError::{ConnectionWsError, FatalWsError, RecoverableWsError};

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
    message_receiver: UnboundedReceiver<WsMessage>,
    status_receiver: UnboundedReceiver<StatusUpdate>,
    control_sender: UnboundedSender<ControlMessage>,
    state: State,
}

#[derive(Debug)]
enum State {
    Started(JoinHandle<()>),
    Stopped,
}

impl StableWebSocket {

    pub fn new(url: Url, subscription: Value) -> anyhow::Result<Self> {
        Self::new_with_timeout(url, subscription, Duration::from_millis(500))
    }
    ///
    /// url: url of the websocket server
    pub fn new_with_timeout(url: Url, subscription: Value, startup_timeout: Duration) -> anyhow::Result<Self> {
        let (message_tx, mut message_rx) = sync::mpsc::unbounded_channel();
        let (sc_tx, mut sc_rx) = sync::mpsc::unbounded_channel();
        let (cc_tx, cc_rx) = sync::mpsc::unbounded_channel();

        // main thread
        let url2 = url.clone();
        let join_handle = tokio::spawn(async move {
            listen_and_handle_reconnects(&url2, startup_timeout, message_tx, sc_tx, cc_rx, &subscription).await;
            return; // loop exhausted by close frame
        });

        // blocking channel and wait for one Subscribed message
        if let Some(StatusUpdate::Subscribed) = block_on(sc_rx.recv()) {
            Ok(Self {
                ws_url: url,
                message_receiver: message_rx,
                status_receiver: sc_rx,
                control_sender: cc_tx,
                state: State::Started(join_handle)
            })
        } else {
            bail!("no subscription success - aborting")
        }

    }

    pub fn get_message_channel(&mut self) -> &mut UnboundedReceiver<WsMessage> {
        &mut self.message_receiver
    }

    pub async fn join(self) {
        match self.state {
            State::Started(join_handle) => {
                join_handle.await.unwrap();
            }
            State::Stopped => {
            }
        }
    }

    pub fn shutdown(&mut self) {
        match &self.state {
            State::Started(join_handle) => {
                self.control_sender.send(ControlMessage::Shutdown).unwrap();
                debug!("shutting down websocket service");

                // wait for shutting down message
                loop {
                    let Some(status) = block_on(self.status_receiver.recv()) else { break; };

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
            State::Stopped => {
                // already stopped - do nothing
            }
        }
    }
}

async fn listen_and_handle_reconnects<T: Serialize>(url: &Url,
                                                    startup_timeout: Duration,
                                                    sender: UnboundedSender<WsMessage>,
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
                    info!("startup timeout reached: {:?}", Instant::now() - start_ts);
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
    url: &Url, sender: &UnboundedSender<WsMessage>, status_sender: &UnboundedSender<StatusUpdate>,
    control_receiver: &mut UnboundedReceiver<ControlMessage>,
    sub: &T) -> Result<(), WebsocketHighLevelError> {
    let (mut ws_stream, response) = connect_async(url).await.map_err(|e| ConnectionWsError(e))?;
    assert_eq!(response.status(), 101, "Error connecting to the server: {:?}", response);

    let shutting_down = Arc::new(AtomicBool::new(false));

    let (mut ws_write, mut ws_read) = ws_stream.split();

    let json_value: Value = json!(sub);
    ws_write.send( tungstenite::Message::text(serde_json::to_string(&json_value).unwrap())).await
        .map_err(|e| map_error(e))?;

    // ping thread - not joined
    let shutting_down_close_stream = shutting_down.clone();
    tokio::spawn(async move {
        let mut interval_shutdown = interval(Duration::from_millis(500));
        let mut interval_ping = interval(Duration::from_millis(1500));
        loop {
            if shutting_down_close_stream.load(Ordering::Relaxed) {
                ws_write.close().await.ok();
                interval_shutdown.tick().await;
            } else {
                ws_write.send(tungstenite::Message::Ping(vec![13,37,42])).await.ok();
                debug!("Websocket Ping sent (period={:?})", interval_ping.period());
                interval_ping.tick().await;
            }
        }


        // ws_read.reunite(ws_write).unwrap().close(None).await.unwrap();
    });

    let mut interval = interval(Duration::from_millis(1500));
    let shutting_down_reconnect = shutting_down.clone();
    let mut subscription_confirmed = false;
    loop {
        select! {
            Some(msg) = ws_read.next() => {
                 match msg.map_err(|e| map_error(e))? {
                    tungstenite::Message::Text(s) => {
                        if !subscription_confirmed && is_subscription_confirmed_message(s.as_str()) {
                            debug!("Subscription confirmed");
                            subscription_confirmed = true;
                            status_sender.send(StatusUpdate::Subscribed).expect("Can't send to channel");
                            continue;
                        }
                        if shutting_down_reconnect.load(Ordering::Relaxed) {
                            debug!("Received Text - but shutting down");
                            continue;
                        }
                        debug!("Received Text: {}", s);
                        sender.send(WsMessage::Text(s.clone())).expect("Can't send to channel");
                    }
                    tungstenite::Message::Binary(data) => {
                        debug!("Received Binary: {} bytes", data.len());
                        if shutting_down_reconnect.load(Ordering::Relaxed) {
                            debug!("Received Binary - but shutting down");
                            continue;
                        }
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
                        match shutting_down_reconnect.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed) {
                            Ok(_) => {
                                info!("Set shutdown flag");
                                status_sender.send(StatusUpdate::ShuttingDown).expect("Can't send to channel");
                            }
                            Err(_) => {
                                debug!("Shutdown flag already set");
                            }
                        }
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
