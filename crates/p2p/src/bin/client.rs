use std::hash::{Hash, Hasher};
use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};
use clap::Parser;
use futures_util::{future, pin_mut, StreamExt};
use hex::ToHex;
use url::Url;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use prost::Message as ProstMessage;
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;
use warp::ws::WebSocket;
use fixt::fixt;
use kitsune_p2p_bin_data::KitsuneAgent;
use kitsune_p2p_types::KAgent;
use protos::message::ZMessage;
use vlc::Clock;
use cops::*;

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
        tx.unbounded_send(Message::binary(buf2)).unwrap();
    }
}

struct VlcClient {
    clock: Option<Clock>,
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>
}

impl VlcClient {
    pub async fn new(url: String) -> Self {
        let url = url::Url::parse(&url).unwrap();
        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        Self {
            clock: None,
            ws: ws_stream,
        }
    }

    pub async fn read(&mut self, key: &str, tx: UnboundedSender<Message>) -> (bool, Option<String>) {
        let request = Request::Read(Read {
            key: key.to_string(),
            clock: self.clock.clone(),
        });
    }

    pub async fn write(&mut self, key: &str, value: &str, index: Option<usize>) {
        let request = Request::Write(Write {
            key: key.into(),
            value: value.into(),
            clock: self.clock.clone(),
        });

        let index = index.unwrap_or_else(|| self.key_to_server(key));

        self.invoke(request, index).await;
    }

    fn key_to_server(&self, key: &str) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() as usize) % 1
    }

    fn update_clock(&mut self, clock: &Clock) {
        if let Some(ref mut my_clock) = self.clock {
            my_clock.merge(&vec![&clock]);
        } else {
            self.clock = Some(clock.clone());
        }
    }

    async fn invoke(&mut self, request: Request, index: usize) -> (bool, Option<String>) {
        self.send_message(request, index).await;
        self.receive_reply().await
    }

    async fn send_message(&mut self, request: Request, index: usize) {
        let msg = serde_json::to_string(&Message::Request(request)).unwrap();
        self.socket
            .send_to(msg.as_bytes(), self.config.server_addrs[index])
            .await
            .unwrap();
        // Todo: add retransmission timeout
        // Todo: add request id
    }

    async fn receive_reply(&mut self) -> (bool, Option<String>) {
        loop {
            let message = self.receive_message().await;
            if let Some(result) = self.process_message(message) {
                return result;
            }
        }
    }

    async fn receive_message(&mut self) -> Message {
        let mut buf = [0; 1500];
        let (len, _) = self.socket.recv_from(&mut buf).await.unwrap();
        serde_json::from_str::<Message>(&String::from_utf8_lossy(&buf[..len])).unwrap()
    }

    fn process_message(&mut self, msg: Message) -> Option<(bool, Option<String>)> {
        match msg {
            Message::Reply(reply) => match reply {
                Reply::ReadReply(reply) => self.process_reply(reply),
                Reply::WriteReply(reply) => self.process_reply(reply),
            },
            _ => None,
        }
    }

    fn process_reply(&mut self, reply: ReadReply) -> Option<(bool, Option<String>)> {
        match reply.clock {
            Some(clock) => {
                self.update_clock(&clock);
                Some((true, reply.value))
            }
            None => Some((false, None)),
        }
    }
}
