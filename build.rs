fn main() {
    connectrpc_build::Config::new()
        .files(&[
            "proto/workers/clock/v1/clock.proto",
            "proto/workers/echo/v1/echo.proto",
            "proto/workers/greet/v1/greet.proto",
            "proto/workers/heartbeat/v1/heartbeat.proto",
            "proto/workers/reverse/v1/reverse.proto",
            "proto/workers/todo/v1/todo.proto",
        ])
        .includes(&["proto"])
        .include_file("_connectrpc.rs")
        .compile()
        .expect("failed to compile protos");
}
