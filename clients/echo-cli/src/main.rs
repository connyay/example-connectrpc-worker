//! ConnectRPC client for `EchoService.Echo`.
//!
//! Usage: `cargo run -p echo-cli -- [URL] [TEXT...]`

use std::env;
use std::process::ExitCode;

use connectrpc::client::{ClientConfig, HttpClient};

mod proto {
    include!(concat!(env!("OUT_DIR"), "/_echo.rs"));
}

use proto::workers::echo::v1::{EchoRequest, EchoServiceClient};

#[tokio::main]
async fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let url = args
        .next()
        .unwrap_or_else(|| "http://localhost:8787".into());
    let mut texts: Vec<String> = args.collect();
    if texts.is_empty() {
        texts = vec!["hello".into(), "world".into(), "!".into()];
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
    let client = EchoServiceClient::new(transport, config);

    let mut stream = match client.echo().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("echo open failed: {e}");
            return ExitCode::from(1);
        }
    };

    for text in &texts {
        if let Err(e) = stream
            .send(EchoRequest {
                text: text.clone(),
                ..Default::default()
            })
            .await
        {
            eprintln!("send failed: {e}");
            break;
        }
        println!("-> {text}");
    }
    stream.close_send();

    let mut received = 0u32;
    loop {
        match stream.message().await {
            Ok(Some(msg)) => {
                println!("<- sequence={} text={:?}", msg.sequence, msg.text);
                received += 1;
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("stream error after {received} response(s): {e}");
                return ExitCode::from(1);
            }
        }
    }
    eprintln!("done; sent {} / received {}", texts.len(), received);
    ExitCode::SUCCESS
}
