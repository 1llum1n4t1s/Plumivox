// AppError は details（Value）を含み、拒否記録の捕捉値の欄を足すと clippy の 128 バイトの目安を超える。
// エラー経路でしか使わず、箱詰めしても欄の分の 8 バイトは減らせないため、この lint だけ許可する。
#![allow(clippy::result_large_err)]
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
// 提供元の拒否文面のローカル記録（既定は無効。`--rejection-log` で期限付きに有効化）。
pub mod rejection_log;
pub mod server;
pub mod session;
pub mod setup;
// Toolport ゲートウェイが既に Plumivox を提供しているかの判定（Remnifold・Edictora と同一内容）。
mod toolport;
#[cfg(windows)]
mod windows_identity;
pub use server::{Options, run_stdio};
#[cfg(feature = "test-fixture")]
pub mod test_support;
