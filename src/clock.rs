//! Server-streaming Clock service.

use buffa::view::OwnedView;
use connectrpc::{ConnectError, RequestContext, Response, ServiceResult, ServiceStream};

use crate::proto::workers::clock::v1::{ClockService, TickRequestView, TickResponse};

pub struct Clock;

const MAX_TICKS: u32 = 1024;

impl ClockService for Clock {
    async fn tick(
        &self,
        _ctx: RequestContext,
        request: OwnedView<TickRequestView<'static>>,
    ) -> ServiceResult<ServiceStream<TickResponse>> {
        let count = request.count;
        if count > MAX_TICKS {
            return Err(ConnectError::invalid_argument(format!(
                "count {count} exceeds maximum of {MAX_TICKS}"
            )));
        }
        let stream = futures::stream::iter((0..count).map(|i| {
            Ok(TickResponse {
                sequence: i,
                ..Default::default()
            })
        }));
        Response::stream_ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as _;
    use futures::executor::block_on;

    use crate::proto::workers::clock::v1::TickRequest;

    fn collect(count: u32) -> Result<Vec<TickResponse>, ConnectError> {
        block_on(async {
            let req = TickRequest {
                count,
                ..Default::default()
            };
            let view = OwnedView::<TickRequestView<'static>>::from_owned(&req).expect("build view");
            let resp = Clock.tick(RequestContext::default(), view).await?;
            resp.body.collect::<Vec<_>>().await.into_iter().collect()
        })
    }

    #[test]
    fn tick_emits_zero_messages_for_zero_count() {
        assert!(collect(0).unwrap().is_empty());
    }

    #[test]
    fn tick_emits_sequential_messages() {
        let out = collect(5).unwrap();
        let seqs: Vec<u32> = out.into_iter().map(|m| m.sequence).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn tick_rejects_excessive_count() {
        let err = block_on(async {
            let req = TickRequest {
                count: MAX_TICKS + 1,
                ..Default::default()
            };
            let view = OwnedView::<TickRequestView<'static>>::from_owned(&req).unwrap();
            Clock.tick(RequestContext::default(), view).await.err()
        })
        .expect("should reject");
        assert_eq!(err.code, connectrpc::ErrorCode::InvalidArgument);
    }
}
