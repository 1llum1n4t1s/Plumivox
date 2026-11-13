#![allow(dead_code)]

mod http;
mod mcp;
mod suite;

// 各integration testは共通APIの必要な部分だけを利用する。
#[allow(unused_imports)]
pub use http::{HttpRequest, HttpResponse, HttpServer, json_response, raw_response};
#[allow(unused_imports)]
pub use mcp::{McpClient, PendingCall};
#[allow(unused_imports)]
pub use suite::{Suite, contains, free_port, redact, wait_until};

pub type Result<T = ()> = anyhow::Result<T>;
