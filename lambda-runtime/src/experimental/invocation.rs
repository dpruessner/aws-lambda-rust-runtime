use std::pin;

use super::*;
use http::Response as Response;

use super::BoxError; // Boxed error

type InvocationOutput = Result<http::Response<Incoming>, BoxError>;

pub struct LambdaInvocationFuture
{
    inner: Pin<Box<dyn Future<Output = InvocationOutput>>>
}
impl LambdaInvocationFuture {
    pub fn new<T: Future<Output = InvocationOutput> + Send + 'static>(f: T) -> Self {
        Self { inner: Box::pin(f) }
    }
}

impl Future for LambdaInvocationFuture
where 
{
    type Output = InvocationOutput;

    fn poll(mut self: pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx)
    }
}

#[derive(Clone)]
pub struct LambaInvocationService {
    client: Arc<Client>,
}
impl LambaInvocationService {
    pub fn new(client: Client) -> Self {
        Self { client: Arc::new(client) }
    }
}

impl<Request> Service<Request> for LambaInvocationService
{
    type Response = http::Response<Incoming>;
    type Error = BoxError;

    type Future = LambdaInvocationFuture;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Request) -> Self::Future {
        let client = self.client.clone();
        let future = async move {
            let req = NextEventRequest.into_req().expect("Unable to construct request");
            let res = client.call(req);
            res.await
        };
        LambdaInvocationFuture::new(future)
    }
}

pub struct ConstantInvocationService {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}
impl ConstantInvocationService {
    pub fn new(status: u16, headers: &[(String, String)], body: &str) -> Self {
        Self {
            status: status,
            headers: headers.to_vec(),
            body: body.to_string(),
        }
    }
}

impl Service<()> for ConstantInvocationService {
    type Response = http::Response<String>;

    type Error = BoxError;

    type Future = futures::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: ()) -> Self::Future {

        let mut response = Response::builder()
            .version(http::Version::HTTP_11)
            .status(self.status);

        for (k, v) in self.headers.iter() {
            response = response.header(k, v);
        }

        let result = response.body(self.body.clone());
        futures::future::ready(result.map_err(|e| e.into()))
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    // Run with:
    //
    //     printf 'HTTP/1.1 200 OK\r\n\r\n' | nc -l -p 9999 
    //
    // to simulate the Lambda Runtime API service
    // 
    async fn test_lambda_invocation_service() {
        let client = Client::builder()
            .with_endpoint("http://127.0.0.1:9999/".parse().unwrap())
            .build()
            .expect("Cannot build client");
        let mut service = super::LambaInvocationService::new(client);
        let req = ();
        let res = service.call(req).await.unwrap();
        assert_eq!(res.status(), 200);
    }

    #[tokio::test]
    async fn test_constant_invocation_service() {
        let mut service = super::ConstantInvocationService::new(
            200,
            &[],
            "Hello, World!"
        );
        let req = ();
        let res = service.call(req).await.unwrap();
        assert_eq!(res.status(), 200);
    }
}