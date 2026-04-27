//! ConnectRPC client for `ClockService.Tick`.
//!
//! Usage: `cargo run -p clock-cli -- [URL] [COUNT]`

use std::env;
use std::process::ExitCode;

use connectrpc::client::{ClientConfig, HttpClient};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/_clock.rs"));
}

use proto::workers::clock::v1::{ClockServiceClient, TickRequest};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8787".into());
    let count: u32 = match args.next() {
        Some(s) => match s.parse() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("invalid count: {e}");
                return ExitCode::from(2);
            }
        },
        None => 5,
    };

    let uri: http::Uri = match url.parse() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("invalid URL {url:?}: {e}");
            return ExitCode::from(2);
        }
    };

    let transport = HttpClient::plaintext();
    let config = ClientConfig::new(uri);
    let client = ClockServiceClient::new(transport, config);

    let mut stream = match client
        .tick(TickRequest {
            count,
            ..Default::default()
        })
        .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("tick call failed: {e}");
            return ExitCode::from(1);
        }
    };

    let mut received = 0u32;
    loop {
        match stream.message().await {
            Ok(Some(msg)) => {
                println!("tick: sequence={}", msg.sequence);
                received += 1;
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("stream error after {received} message(s): {e}");
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("done; received {received} message(s)");
    ExitCode::SUCCESS
}
