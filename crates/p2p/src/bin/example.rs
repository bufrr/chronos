use fixt::prelude::*;
use kitsune_p2p::actor::BroadcastData;
use kitsune_p2p::actor::KitsuneP2pSender;
use kitsune_p2p::fixt::KitsuneSpaceFixturator;
use kitsune_p2p::KitsuneBinType;
use kitsune_p2p_bin_data::{KitsuneAgent, KitsuneBasis, KitsuneSpace};
use kitsune_p2p_fetch::FetchContext;
use kitsune_p2p_types::{KAgent, KitsuneTimeout};
use std::sync::Arc;
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

    let sendto = args.sendto.map(|sendto| {
        let decoded = hex::decode(sendto).unwrap();
        Arc::new(KitsuneAgent(decoded))
    });

    spawn_node_task(&name, &connection_url, &passphrase, signal_url, bootstrap_addr, sendto).await;

    tokio::time::sleep(std::time::Duration::from_secs(3000)).await;
}

async fn spawn_node_task(
    name: &str,
    connection_url: &str,
    passphrase: &str,
    signal_url: SocketAddr,
    bootstrap_addr: SocketAddr,
    sendto: Option<KAgent>,
) {
    let connection_url = connection_url.to_owned();
    let passphrase = passphrase.to_owned();
    let name = name.to_owned();

    tokio::spawn(async move {
        run_node(&name, &connection_url, &passphrase, signal_url, bootstrap_addr, sendto).await;
    });
}

async fn run_node(
    name: &str,
    connection_url: &str,
    passphrase: &str,
    signal_url: SocketAddr,
    bootstrap_addr: SocketAddr,
    sendto: Option<KAgent>,
) -> () {
    let mut harness = KitsuneTestHarness::try_new(name, passphrase, connection_url)
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

    if let Some(to) = sendto {
        for i in 0..100 {
            let response = sender.rpc_single(
                space.clone(),
                to.clone(),
                i.to_string().as_bytes().to_vec(),
                Some(5_000),
            ).await.unwrap();

            println!("Response: {:?}", String::from_utf8(response));
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    tokio::time::sleep(std::time::Duration::from_secs(3000)).await;
}