fn main() {
    connectrpc_build::Config::new()
        .files(&["../../proto/workers/heartbeat/v1/heartbeat.proto"])
        .includes(&["../../proto"])
        .include_file("_heartbeat.rs")
        .compile()
        .expect("failed to compile heartbeat.proto");
}
