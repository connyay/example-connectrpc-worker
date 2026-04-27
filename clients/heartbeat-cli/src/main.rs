//! Full-duplex probe for `HeartbeatService.Heartbeat`.
//!
//! Usage: `cargo run -p heartbeat-cli -- [URL] [INTER_SEND_MS] [NOTE...]`

use std::env;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use connectrpc::client::{ClientConfig, HttpClient};
use tokio::time::sleep;

mod proto {
    include!(concat!(env!("OUT_DIR"), "/_heartbeat.rs"));
}

use proto::workers::heartbeat::v1::{HeartbeatRequest, HeartbeatServiceClient};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8787".into());
    let inter_send_ms: u64 = match args.next() {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("invalid inter-send delay: {e}");
                return ExitCode::from(2);
            }
        },
        None => 300,
    };
    let mut notes: Vec<String> = args.collect();
    if notes.is_empty() {
        notes = vec!["one".into(), "two".into(), "three".into()];
    }

    let uri: http::Uri = match url.parse() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("invalid URL {url:?}: {e}");
            return ExitCode::from(2);
        }
    };

    let transport = HttpClient::plaintext();
    let config = ClientConfig::new(uri);
    let client = HeartbeatServiceClient::new(transport, config);

    let mut stream = match client.heartbeat().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("heartbeat open failed: {e}");
            return ExitCode::from(1);
        }
    };

    let t0 = Instant::now();
    let stamp = || (t0.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0;

    match stream.message().await {
        Ok(Some(msg)) => {
            println!(
                "[{:>8.2} ms] <- sequence={} note={:?} (initial)",
                stamp(),
                msg.sequence,
                msg.note,
            );
        }
        Ok(None) => {
            eprintln!("stream closed before initial pong");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("error awaiting initial pong: {e}");
            return ExitCode::from(1);
        }
    }

    for note in &notes {
        sleep(Duration::from_millis(inter_send_ms)).await;
        if let Err(e) = stream
            .send(HeartbeatRequest {
                note: note.clone(),
                ..Default::default()
            })
            .await
        {
            eprintln!("[{:>8.2} ms] send failed: {e}", stamp());
            break;
        }
        println!("[{:>8.2} ms] -> {note}", stamp());

        match stream.message().await {
            Ok(Some(msg)) => println!(
                "[{:>8.2} ms] <- sequence={} note={:?}",
                stamp(),
                msg.sequence,
                msg.note,
            ),
            Ok(None) => {
                eprintln!("[{:>8.2} ms] stream closed mid-call", stamp());
                return ExitCode::from(1);
            }
            Err(e) => {
                eprintln!("[{:>8.2} ms] error awaiting echo: {e}", stamp());
                return ExitCode::from(1);
            }
        }
    }

    stream.close_send();
    println!("[{:>8.2} ms] -- close_send", stamp());

    loop {
        match stream.message().await {
            Ok(Some(msg)) => println!(
                "[{:>8.2} ms] <- sequence={} note={:?}",
                stamp(),
                msg.sequence,
                msg.note,
            ),
            Ok(None) => break,
            Err(e) => {
                eprintln!("[{:>8.2} ms] drain error: {e}", stamp());
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("done; sent {}", notes.len());
    ExitCode::SUCCESS
}
