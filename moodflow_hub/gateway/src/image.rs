//! Explicit in-memory image inputs. No URLs, paths, metadata fetching or Debug output.
use base64::Engine;
use moodflow_hub_core::infra::gateway::AiError;
use zeroize::Zeroizing;

pub struct InlineImage {
    data: Zeroizing<String>,
}
impl InlineImage {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AiError> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(AiError::BadRequest);
        }
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else {
            return Err(AiError::BadRequest);
        };
        Ok(Self {
            data: Zeroizing::new(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )),
        })
    }
    pub(crate) fn data_url(&self) -> &str {
        &self.data
    }
}
