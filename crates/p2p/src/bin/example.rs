use fixt::prelude::*;
use kitsune_p2p::actor::BroadcastData;
use kitsune_p2p::actor::KitsuneP2pSender;
use kitsune_p2p::fixt::KitsuneSpaceFixturator;
use kitsune_p2p::KitsuneBinType;
use kitsune_p2p_bin_data::{KitsuneAgent, KitsuneBasis, KitsuneSpace};
use kitsune_p2p_fetch::FetchContext;
use kitsune_p2p_types::{KAgent, KitsuneTimeout};
use std::sync::{Arc, Mutex};
use kitsune_p2p_types::dependencies::holochain_trace;
use kitsune_p2p_types::dependencies::rustls::internal::msgs::base::Payload;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::thread::spawn;
use hex::ToHex;
use rusqlite::Connection;
use serde_json::to_vec;
use p2p::common::{KitsuneTestHarness, RecordedKitsuneP2pEvent, start_bootstrap, start_signal_srv, TestHostOp, wait_for_connected};
use kitsune_p2p::dht::hash::hash_slice_32;

use clap::Parser;

use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::RwLock;
use warp::Filter;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use warp::ws::{Message, WebSocket};

use protos::message::ZMessage;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    name: String,
    connection_url: String,
    passphrase: String,
    signal_url: String,
    bootstrap_url: String,

    #[arg(short, long)]
    listen_url: String,

    #[arg(short, long)]
    sendto: Option<String>,
}

#[cfg(feature = "tx5")]
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();

    let name = args.name.to_owned();
    let passphrase = args.passphrase.to_owned();
    let connection_url = args.connection_url.to_owned();
    let addr_bootstrap = args.bootstrap_url.to_owned();
    let signal_url = args.signal_url.to_owned();
    let bootstrap_addr: SocketAddr = addr_bootstrap.parse().unwrap();
    let signal_url: SocketAddr = signal_url.parse().unwrap();
    let listen_url: SocketAddr = args.listen_url.parse().unwrap();

    let (tx1, rx1) = tokio::sync::mpsc::unbounded_channel();
    let (tx2, rx2) = tokio::sync::mpsc::unbounded_channel();
    let rx2 = Arc::new(RwLock::new(rx2));


    spawn_node_task(&name, &connection_url, &passphrase, signal_url, bootstrap_addr, rx1, tx2).await;

    let routes = warp::path("vlc")
        // The `ws()` filter will prepare Websocket handshake...
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            // This will call our function if the handshake succeeds.
            let tx = tx1.clone();
            let rx2 = rx2.clone();
            ws.on_upgrade(move |websocket| pipe(websocket, tx, rx2))
        });
    warp::serve(routes).run(listen_url).await;
}

async fn pipe(
    ws: WebSocket,
    tx: UnboundedSender<Vec<u8>>,
    rx: Arc<RwLock<UnboundedReceiver<Vec<u8>>>>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();

    let receiver_task = tokio::spawn(async move {
        let mut rx = rx.write().await;
        while let Some(msg) = rx.recv().await {
            if let Err(e) = ws_tx.send(Message::binary(msg)).await {
                eprintln!("Failed to send WebSocket message: {}", e);
                break;
            }
        }
    });

    let sender_task = tokio::spawn(async move {
        while let Some(result) = ws_rx.next().await {
            match result {
                Ok(msg)  => {
                    tx.send(msg.into_bytes()).expect("Failed to send message");
                }
                Err(e) => {
                    eprintln!("WebSocket error: {}", e);
                    break;
                }
            }
        }
    });

    let _ = tokio::try_join!(receiver_task, sender_task);
}

async fn spawn_node_task(
    name: &str,
    connection_url: &str,
    passphrase: &str,
    signal_url: SocketAddr,
    bootstrap_addr: SocketAddr,
    rx: UnboundedReceiver<Vec<u8>>,
    tx: UnboundedSender<Vec<u8>>,
) {
    let connection_url = connection_url.to_owned();
    let passphrase = passphrase.to_owned();
    let name = name.to_owned();

    tokio::spawn(async move {
        run_node(&name, &connection_url, &passphrase, signal_url, bootstrap_addr, rx, tx).await;
    });
}

async fn run_node(
    name: &str,
    connection_url: &str,
    passphrase: &str,
    signal_url: SocketAddr,
    bootstrap_addr: SocketAddr,
    mut rx: UnboundedReceiver<Vec<u8>>,
    tx: UnboundedSender<Vec<u8>>,
) -> () {
    use prost::Message;

    let mut harness = KitsuneTestHarness::try_new(name, passphrase, connection_url, tx)
        .await
        .expect("Failed to setup test harness")
        .configure_tx5_network(signal_url)
        .use_bootstrap_server(bootstrap_addr)
        .update_tuning_params(|mut c| {
            // 3 seconds between gossip rounds, to keep the test fast
            c.gossip_peer_on_success_next_gossip_delay_ms = 1000 * 3;
            c
        });

    let sender = harness
        .spawn()
        .await
        .expect("should be able to spawn node");

    let agent = harness.get_agent(name).await;

    let hex_a: String = agent.encode_hex::<String>();
    println!("Agent: {:?}", hex_a);

    let space_bytes: Vec<u8> = vec![0; 36];
    let space = Arc::new(KitsuneSpace::new(space_bytes));

    sender.join(space.clone(), agent.clone(), None, None).await.unwrap();
    //wait_for_connected(sender.clone(), agent.clone(), space.clone()).await;

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;


    while let Some(msg) = rx.recv().await {
        let msg = prost::bytes::Bytes::from(msg);
        let m = ZMessage::decode(msg.clone()).unwrap();
        let decoded = hex::decode(m.to.clone()).unwrap();
        let to = Arc::new(KitsuneAgent(decoded));
        let response = sender.rpc_single(
            space.clone(),
            to.clone(),
            msg.to_vec(),
            Some(5_000),
        ).await.unwrap();

        println!("Response: {:?}", String::from_utf8(response));
    }
}