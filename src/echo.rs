//! Bidi-streaming Echo service.

use std::pin::Pin;

use buffa::view::OwnedView;
use connectrpc::{ConnectError, Context as RpcContext};
use futures::{Stream, StreamExt as _};

use crate::proto::workers::echo::v1::{EchoRequestView, EchoResponse, EchoService};

pub struct Echoer;

impl EchoService for Echoer {
    async fn echo(
        &self,
        ctx: RpcContext,
        requests: Pin<
            Box<
                dyn Stream<Item = Result<OwnedView<EchoRequestView<'static>>, ConnectError>> + Send,
            >,
        >,
    ) -> Result<
        (
            Pin<Box<dyn Stream<Item = Result<EchoResponse, ConnectError>> + Send>>,
            RpcContext,
        ),
        ConnectError,
    > {
        let responses = futures::stream::unfold((requests, 0u32), |(mut s, seq)| async move {
            match s.next().await {
                Some(Ok(view)) => {
                    let resp = EchoResponse {
                        text: view.text.to_owned(),
                        sequence: seq,
                        ..Default::default()
                    };
                    Some((Ok(resp), (s, seq + 1)))
                }
                Some(Err(e)) => Some((Err(e), (s, seq))),
                None => None,
            }
        });
        Ok((Box::pin(responses), ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    use crate::proto::workers::echo::v1::EchoRequest;

    fn run(texts: &[&str]) -> Vec<EchoResponse> {
        block_on(async {
            let owned: Vec<EchoRequest> = texts
                .iter()
                .map(|t| EchoRequest {
                    text: (*t).to_owned(),
                    ..Default::default()
                })
                .collect();
            let views: Vec<Result<OwnedView<EchoRequestView<'static>>, ConnectError>> = owned
                .iter()
                .map(|r| Ok(OwnedView::<EchoRequestView<'static>>::from_owned(r).unwrap()))
                .collect();
            let stream = futures::stream::iter(views);
            let (out, _ctx) = Echoer
                .echo(RpcContext::default(), Box::pin(stream))
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
    fn echo_pairs_each_input_with_a_sequenced_response() {
        let out = run(&["hello", "world", "!"]);
        let pairs: Vec<(String, u32)> = out.into_iter().map(|r| (r.text, r.sequence)).collect();
        assert_eq!(
            pairs,
            vec![("hello".into(), 0), ("world".into(), 1), ("!".into(), 2),]
        );
    }

    #[test]
    fn echo_emits_nothing_when_client_sends_nothing() {
        assert!(run(&[]).is_empty());
    }
}
