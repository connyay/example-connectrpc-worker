fn main() {
    connectrpc_build::Config::new()
        .files(&["../../proto/workers/clock/v1/clock.proto"])
        .includes(&["../../proto"])
        .include_file("_clock.rs")
        .compile()
        .expect("failed to compile clock.proto");
}
