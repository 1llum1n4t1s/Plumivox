mod agent_dirs;
mod api_rejection;
// Setup（Velopack）導入版の起動時処理・自動更新・Claude Code/Codex への登録。
pub mod app_update;
pub mod client;
pub mod config;
mod connect;
pub mod error;
// 人が読む文言（CLI・セットアップ・Setup 版の案内）の9言語辞書と言語判定。MCP の経路からは使わない。
pub mod locale;
pub mod proxy;
mod proxy_egress;
mod proxy_ipc;
pub mod server;
pub mod session;
pub mod setup;
#[cfg(windows)]
mod windows_identity;
pub use server::{Options, run_stdio};
#[cfg(feature = "test-fixture")]
pub mod test_support;
