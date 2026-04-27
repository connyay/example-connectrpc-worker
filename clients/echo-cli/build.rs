fn main() {
    connectrpc_build::Config::new()
        .files(&["../../proto/workers/echo/v1/echo.proto"])
        .includes(&["../../proto"])
        .include_file("_echo.rs")
        .compile()
        .expect("failed to compile echo.proto");
}
