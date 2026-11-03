use super::Result;
use anyhow::Context;
use http_body_util::{BodyExt, Full, Limited, combinators::BoxBody};
use hyper::{
    HeaderMap, Request, Response,
    body::{Bytes, Incoming},
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::{future::Future, io, sync::Arc};
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

pub type HttpResponse = Response<BoxBody<Bytes, io::Error>>;

pub fn raw_response(status: u16, bytes: Vec<u8>) -> HttpResponse {
    Response::builder()
        .status(status)
        .body(
            Full::new(Bytes::from(bytes))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("valid test HTTP status")
}

pub fn json_response(status: u16, value: Value) -> HttpResponse {
    let mut response = raw_response(status, serde_json::to_vec(&value).expect("JSON response"));
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}

pub struct HttpServer {
    pub port: u16,
    pub base_url: String,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl HttpServer {
    pub async fn start<F, Fut>(handler: F) -> Result<Self>
    where
        F: Fn(HttpRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = io::Result<HttpResponse>> + Send + 'static,
    {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .context("Cannot bind fake HTTP server")?;
        let port = listener.local_addr()?.port();
        let stop = CancellationToken::new();
        let closing = stop.clone();
        let handler = Arc::new(handler);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = closing.cancelled() => break,
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break };
                        let handler = handler.clone();
                        let stopping = closing.clone();
                        connections.spawn(async move {
                            let service = service_fn(move |request: Request<Incoming>| {
                                let handler = handler.clone();
                                async move {
                                    let (head, body) = request.into_parts();
                                    let body = Limited::new(body, 32 * 1024 * 1024).collect().await
                                        .map_err(|error| io::Error::other(error.to_string()))?.to_bytes().to_vec();
                                    handler(HttpRequest { method: head.method.to_string(),
                                        path: head.uri.path().to_owned(), query: head.uri.query().unwrap_or("").to_owned(),
                                        headers: head.headers, body }).await
                                }
                            });
                            // handlerのErrはHTTPエラー応答へ変換せず実際に接続を切る。
                            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
                            tokio::select! { _ = stopping.cancelled() => {}, _ = connection => {} }
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        Ok(Self {
            port,
            base_url: format!("http://127.0.0.1:{port}"),
            stop,
            task: Some(task),
        })
    }

    pub async fn close(&mut self) -> Result<()> {
        self.stop.cancel();
        if let Some(mut task) = self.task.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut task).await {
                Ok(result) => result.context("Fake HTTP server task failed")?,
                Err(_) => task.abort(),
            }
        }
        Ok(())
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
