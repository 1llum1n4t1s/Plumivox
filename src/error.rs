use serde_json::{Value, json};

pub type Result<T> = std::result::Result<T, AppError>;

/// 公開用の固定エラーだけを保持し、OS/APIの生エラーを境界へ持ち出さない。
#[derive(Debug, Clone)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub details: Value,
}
impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
    pub fn value(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if let Some(details) = self.details.as_object() {
            for (key, value) in details {
                error[key] = value.clone();
            }
        }
        error
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
impl std::error::Error for AppError {}
