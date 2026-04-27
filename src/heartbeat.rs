//! Full-duplex Heartbeat service: emits an "open" pong before reading any
//! request, echoes each request, then emits a "close" pong on EOF.

use std::pin::Pin;

use buffa::view::OwnedView;
use connectrpc::{ConnectError, Context as RpcContext};
use futures::{Stream, StreamExt as _};

use crate::proto::workers::heartbeat::v1::{
    HeartbeatRequestView, HeartbeatResponse, HeartbeatService,
};

pub struct Heartbeat;

enum State {
    EmitInitial,
    Echoing { seq: u32 },
    Done,
}

impl HeartbeatService for Heartbeat {
    async fn heartbeat(
        &self,
        ctx: RpcContext,
        requests: Pin<
            Box<
                dyn Stream<Item = Result<OwnedView<HeartbeatRequestView<'static>>, ConnectError>>
                    + Send,
            >,
        >,
    ) -> Result<
        (
            Pin<Box<dyn Stream<Item = Result<HeartbeatResponse, ConnectError>> + Send>>,
            RpcContext,
        ),
        ConnectError,
    > {
        let stream = futures::stream::unfold(
            (requests, State::EmitInitial),
            |(mut s, state)| async move {
                match state {
                    State::EmitInitial => {
                        let resp = HeartbeatResponse {
                            sequence: 0,
                            note: "open".into(),
                            ..Default::default()
                        };
                        Some((Ok(resp), (s, State::Echoing { seq: 1 })))
                    }
                    State::Echoing { seq } => match s.next().await {
                        Some(Ok(view)) => {
                            let resp = HeartbeatResponse {
                                sequence: seq,
                                note: view.note.to_owned(),
                                ..Default::default()
                            };
                            Some((Ok(resp), (s, State::Echoing { seq: seq + 1 })))
                        }
                        Some(Err(e)) => Some((Err(e), (s, State::Done))),
                        None => {
                            let resp = HeartbeatResponse {
                                sequence: seq,
                                note: "close".into(),
                                ..Default::default()
                            };
                            Some((Ok(resp), (s, State::Done)))
                        }
                    },
                    State::Done => None,
                }
            },
        );
        Ok((Box::pin(stream), ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    use crate::proto::workers::heartbeat::v1::HeartbeatRequest;

    fn run(notes: &[&str]) -> Vec<HeartbeatResponse> {
        block_on(async {
            let owned: Vec<HeartbeatRequest> = notes
                .iter()
                .map(|n| HeartbeatRequest {
                    note: (*n).to_owned(),
                    ..Default::default()
                })
                .collect();
            let views: Vec<Result<OwnedView<HeartbeatRequestView<'static>>, ConnectError>> = owned
                .iter()
                .map(|r| Ok(OwnedView::<HeartbeatRequestView<'static>>::from_owned(r).unwrap()))
                .collect();
            let req_stream = futures::stream::iter(views);
            let (out, _ctx) = Heartbeat
                .heartbeat(RpcContext::default(), Box::pin(req_stream))
                .await
                .unwrap();
            out.collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        })
    }

    #[test]
    fn heartbeat_emits_open_then_echoes_then_close() {
        let out = run(&["a", "b"]);
        let pairs: Vec<(u32, String)> = out.into_iter().map(|r| (r.sequence, r.note)).collect();
        assert_eq!(
            pairs,
            vec![
                (0, "open".into()),
                (1, "a".into()),
                (2, "b".into()),
                (3, "close".into()),
            ]
        );
    }

    #[test]
    fn heartbeat_emits_open_then_close_when_no_inputs() {
        let out = run(&[]);
        let pairs: Vec<(u32, String)> = out.into_iter().map(|r| (r.sequence, r.note)).collect();
        assert_eq!(pairs, vec![(0, "open".into()), (1, "close".into())]);
    }
}
