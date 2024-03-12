#![allow(unused_imports, dead_code, missing_docs)]

use ::tracing::{error, trace, Instrument};
use bytes::Bytes;
use futures::{future::BoxFuture, FutureExt};
use http_body_util::BodyExt;
use hyper::{body::Incoming, http::Request};
use lambda_runtime_api_client::{body::Body, BoxError, Client};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    env,
    fmt::{self, Debug},
    future::Future,
    panic,
    sync::Arc,
    task::Poll,
};
use tokio_stream::{Stream, StreamExt};
pub use tower::{self, service_fn, Service, Layer};
use tower::{
    ServiceExt,
    layer::layer_fn,
    util::ServiceFn,
};

use crate::requests::NextEventRequest;

use super::{
    Config,
    Diagnostic,
    Error,
    IntoFunctionResponse,
    IntoRequest,
};

pub mod invocation;
pub mod parse;


/// Raw invocation from the Runtime APIs.
pub struct Invocation {
    // The http Response Parts received from Lambda
    pub parts: http::response::Parts,
    // The http Response body received from Lambda
    pub body: hyper::body::Incoming,
}

#[derive(Serialize)]
pub struct LambdaEvent<T>
where
    T: for<'de> Deserialize<'de>,
{
    body: T,
}


pub struct ExperimentalRuntime //<F, A>
where 
    // F: Service<LambdaEvent<A>>,
    // F::Error: Into<BoxError> + Debug,
    // A: for<'de> Deserialize<'de>,
{
    client: super::Client,
    config: super::RefConfig,
    // layer: Box<dyn Layer<F, Service = F>>,
    // _phantom: std::marker::PhantomData<A>,
}

impl ExperimentalRuntime 
// impl<F, A> ExperimentalRuntime<F, A> 
where 
    // F: Service<LambdaEvent<A>>,
    // F::Error: Into<BoxError> + Debug,
    // A: for<'de> Deserialize<'de>,
{
    /// Create the runtime to be used with layers
    pub fn initialize<F, A, R, B, S, D, E>(handler_service: F) -> Self
    where
        F: Service<LambdaEvent<A>>,
        F::Error: Into<BoxError> + Debug,
        A: for<'de> Deserialize<'de>,
        F::Future: Future<Output = Result<R, F::Error>>,
        F::Error: for<'a> Into<Diagnostic<'a>> + fmt::Debug,
        R: IntoFunctionResponse<B, S>,
        B: Serialize,
        S: Stream<Item = Result<D, E>> + Unpin + Send + 'static,
        D: Into<Bytes> + Send,
        E: Into<Error> + Send + Debug,
    {
        trace!("Loading config from env");
        let config = Config::from_env();
        let client = Client::builder().build().expect("Unable to create a runtime client");

        let layer = Box::new(layer_fn(handler_service));
        

        let runtime = ExperimentalRuntime {
            client,
            config: Arc::new(config),
            // layer: layer,
            // _phantom: Default::default(),
        };
        runtime
    }
}


struct InvocationEventsService {
    client: Arc<Client>,
}

use std::pin::Pin;

// Implement Tower Service for the client
impl<Request> Service<Request> for InvocationEventsService {
    type Response = http::Response<Incoming>;

    type Error = Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request) -> Self::Future {
        trace!("Waiting for next event (incoming loop)");
        let req = NextEventRequest.into_req().expect("Unable to construct request");

        let client = self.client.clone();
        Box::pin(async move { 
            client.call(req).await 
        })
    }
}






#[cfg(test)]
mod test {
    type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
    type LambdaError = BoxError;

    use std::{future::Pending, task::Poll, time::SystemTime};

    // Test layering system 
    use tower::{ 
        Layer, Service, service_fn, 
        layer::layer_fn,
    };
    use serde::{Serialize, Deserialize};
    use serde_json::{json, Value};
    use hyper::{
        Response,
        body::{Body, Incoming},
    };
    use bytes::Bytes;
    use futures::{Stream, StreamExt};

    use async_stream::stream;
    use http_body_util::BodyStream;
    use http_body_util::BodyExt;

    // Start simple, we just use Value as everything

    struct MyRuntime {
    }


    fn make_response() -> Result<Response<impl Body>, LambdaError> {
        // Return as a single chunk
        let body = json!({ "hello": "world" }).to_string();
        let request_id = "00000001-0000-0000-0000-000000000001".to_owned();
        let function_arn = "arn:aws:lambda:us-east-2-123456789012:function:custom-runtime".to_owned();
        let trace_id = "Root=1-5759e988-bd862e3fe1be46a994272793;Parent=53995c3f42cd8ad8;Sampled=1".to_owned();
        // get unix timestamp as millis in Unix Epoch
        let deadline_ms = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis().to_string();

        let response = Response::builder()
            .status(200)
            .header("Content-Type", "application/json")
            .header("Lambda-Runtime-Aws-Request-Id", request_id)
            .header("Lambda-Runtime-Deadline-Ms", deadline_ms)
            .header("Lambda-Runtime-Invoked-Function-Arn", function_arn)
            .header("Lambda-Runtime-Trace-Id", trace_id)
            .body(body);

        response.map_err(|e| e.into())
    }

    fn incoming() -> impl Stream<Item = Result<http::Response<impl Body>, LambdaError>> + Send {
        async_stream::stream! {
            loop {
                tracing::log::trace!("Generating next repsonse");
                let res = make_response();
                yield res;
            }
        }
    }

    struct Invocation<D> 
    {
        value: D
    }

    // Test converting a 

    struct ParsingService<D> {
        _phantom: std::marker::PhantomData<D>,
    }
    impl<D> Default for ParsingService<D> {
        fn default() -> Self {
            Self { _phantom: Default::default() }
        }
    }

    impl<B, D> Service<Result<http::Request<B>, LambdaError>> for ParsingService<D>
    where
        B: Body + Send + Sync + 'static,
        B::Data: Send + Sync,
        B::Error: Into<BoxError>,
        D: for<'a> Deserialize<'a>
    {
        type Response = Invocation<D>;
        type Error = LambdaError;
        type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;
        
        fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        
        fn call(&mut self, req: Result<http::Request<B>, LambdaError>) -> Self::Future {
            Box::pin(async {
                let body = req?.into_body();
                let body_data = body
                    .collect()
                    .await
                    .map_err(|e| e.into())?
                    .to_bytes();
                let value = serde_json::from_slice::<D>(&body_data)?;
                Ok(Invocation { value })
            })
        }
    }

    // Create a result of the service that itself is a Future 
    struct Something {}

    struct ParsingLayer;
    impl<S> Layer<S> for ParsingLayer {
        type Service = ParsingService<S>;

        fn layer(&self, inner: S) -> Self::Service {
            ParsingService { .. Default::default() }
        }
    }

    // #[tokio::test]
    // async fn test_conversion() {
    //     let mut stream = Box::pin(incoming());

    //     let res = stream.next().await.unwrap();
    //     let layer = ParsingLayer{};
    //     let service = layer.layer(res);

    //     let value = res.into_parts().body.into_data().await.unwrap();
    //     assert_eq!(value.value, json!({ "hello": "world" }));
    // }

}