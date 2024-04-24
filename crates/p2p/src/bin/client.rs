use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};
use clap::Parser;
use futures_util::{future, pin_mut, StreamExt};
use hex::ToHex;
use url::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use prost::Message as ProstMessage;
use fixt::fixt;
use kitsune_p2p_bin_data::KitsuneAgent;
use kitsune_p2p_types::KAgent;
use protos::message::ZMessage;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    wsaddr: String,

    #[arg(short, long)]
    from: String,

    #[arg(short, long)]
    to: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let connect_addr = args.wsaddr.to_owned();
    let from = args.from.to_owned();
    let to = args.to.to_owned();

    let url = url::Url::parse(&connect_addr).unwrap();

    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin(stdin_tx, from, to));

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, read) = ws_stream.split();

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            let zmsg = ZMessage::decode(data.as_slice()).unwrap();
            let agent_hex: String = String::from_utf8(zmsg.from).unwrap();
            println!("from : {:?}, data: {:?}", agent_hex, String::from_utf8(zmsg.data).unwrap());
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(tx: futures_channel::mpsc::UnboundedSender<Message>, from: String, to: String) {
    let mut stdin = tokio::io::stdin();
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n-1);
        if buf.is_empty() {
            continue;
        }
        let msg = ZMessage {
            id: vec![1,2,3,4,5,6],
            from: Vec::from(from.clone()),
            to: Vec::from(to.clone()),
            data: buf,
            ..Default::default()
        };
        let mut buf2 = vec![];
        msg.encode(&mut buf2).unwrap();
        println!("buf: {:?}", buf2);
        tx.unbounded_send(Message::binary(buf2)).unwrap();
    }
}
