//! Buffer 投递扩展的有界线协议。正文不得进入日志。
//!
//! `buffer.deliver`: id:u64 + client:u64 + revision:u64 + bytes:u32 + UTF-8。
//! `buffer.receipt`: id:u64 + client:u64 + revision:u64 + outcome:u8。
//! `buffer.mode`: client:u64 + revision:u64 (0 disables) + generation:u64。
//! 所有整数小端，布局镜像为 TSF `BufferDelivery.h`。旧宿主忽略扩展，服务端超时保留内容。

pub const MAX_TEXT_UTF16: usize = 16_384;
pub const DELIVERY_TIMEOUT_MS: u64 = 5_000;

/// Capture applies only to this client and input context. Revision 0 disables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub client: u64,
    pub revision: u64,
    pub generation: u64,
}
impl Mode {
    pub fn encode(self) -> Vec<u8> {
        [
            self.client.to_le_bytes(),
            self.revision.to_le_bytes(),
            self.generation.to_le_bytes(),
        ]
        .concat()
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        (bytes.len() == 24).then(|| Self {
            client: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            revision: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            generation: u64::from_le_bytes(bytes[16..].try_into().unwrap()),
        })
    }
}

#[cfg(test)]
#[test]
fn mode_codec_requires_exact_identity_and_generation() {
    let mode = Mode {
        client: 17,
        revision: 29,
        generation: 3,
    };
    let bytes = mode.encode();
    assert_eq!(Mode::decode(&bytes), Some(mode));
    for length in 0..24 {
        assert!(Mode::decode(&bytes[..length]).is_none());
    }
    let mut extra = bytes;
    extra.push(0);
    assert!(Mode::decode(&extra).is_none());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Accepted = 0,
    Rejected = 1,
    Unknown = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub id: u64,
    pub client: u64,
    pub revision: u64,
    pub outcome: ReceiptOutcome,
}

impl Receipt {
    pub fn decode(body: &[u8]) -> Option<Self> {
        if body.len() != 25 {
            return None;
        }
        Some(Self {
            id: read_u64(body, 0)?,
            client: read_u64(body, 8)?,
            revision: read_u64(body, 16)?,
            outcome: match body[24] {
                0 => ReceiptOutcome::Accepted,
                1 => ReceiptOutcome::Rejected,
                2 => ReceiptOutcome::Unknown,
                _ => return None,
            },
        })
    }

    pub fn encode(self) -> Vec<u8> {
        let mut body = Vec::with_capacity(25);
        body.extend_from_slice(&self.id.to_le_bytes());
        body.extend_from_slice(&self.client.to_le_bytes());
        body.extend_from_slice(&self.revision.to_le_bytes());
        body.push(self.outcome as u8);
        body
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Delivery {
    pub id: u64,
    pub client: u64,
    pub revision: u64,
    pub text: String,
}

impl Delivery {
    pub fn encode(&self) -> Option<Vec<u8>> {
        if !valid_text(&self.text) || self.revision == 0 {
            return None;
        }
        let mut body = Vec::with_capacity(28 + self.text.len());
        body.extend_from_slice(&self.id.to_le_bytes());
        body.extend_from_slice(&self.client.to_le_bytes());
        body.extend_from_slice(&self.revision.to_le_bytes());
        body.extend_from_slice(&(self.text.len() as u32).to_le_bytes());
        body.extend_from_slice(self.text.as_bytes());
        Some(body)
    }

    pub fn decode(body: &[u8]) -> Option<Self> {
        let bytes = u32::from_le_bytes(body.get(24..28)?.try_into().ok()?) as usize;
        if bytes > MAX_TEXT_UTF16 * 3 || body.len() != 28 + bytes {
            return None;
        }
        let text = std::str::from_utf8(body.get(28..)?).ok()?;
        let revision = read_u64(body, 16)?;
        if !valid_text(text) || revision == 0 {
            return None;
        }
        Some(Self {
            id: read_u64(body, 0)?,
            client: read_u64(body, 8)?,
            revision,
            text: text.to_owned(),
        })
    }
}

fn valid_text(text: &str) -> bool {
    !text.is_empty() && !text.contains('\0') && text.encode_utf16().count() <= MAX_TEXT_UTF16
}

fn read_u64(body: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        body.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_preserves_unicode_and_rejects_truncation_or_extra_bytes() {
        let delivery = Delivery {
            id: 7,
            client: 0x123456789,
            revision: 3,
            text: "甲🙂\n乙".into(),
        };
        let body = delivery.encode().unwrap();
        assert!(Delivery::decode(&body).is_some_and(|d| d == delivery));
        for end in 0..body.len() {
            assert!(Delivery::decode(&body[..end]).is_none());
        }
        let mut extra = body;
        extra.push(0);
        assert!(Delivery::decode(&extra).is_none());
    }

    #[test]
    fn delivery_is_bounded_in_utf16_and_requires_a_field_revision() {
        let mut delivery = Delivery {
            id: 1,
            client: 2,
            revision: 1,
            text: "🙂".repeat(8192),
        };
        assert!(delivery.encode().is_some());
        delivery.text.push('a');
        assert!(delivery.encode().is_none());
        delivery.text = "a\0b".into();
        assert!(delivery.encode().is_none());
        delivery.text = "a".into();
        delivery.revision = 0;
        assert!(delivery.encode().is_none());
    }

    #[test]
    fn receipts_are_exact_and_unknown_values_are_not_success() {
        for outcome in [
            ReceiptOutcome::Accepted,
            ReceiptOutcome::Rejected,
            ReceiptOutcome::Unknown,
        ] {
            let receipt = Receipt {
                id: 6,
                client: 9,
                revision: 4,
                outcome,
            };
            assert_eq!(Receipt::decode(&receipt.encode()), Some(receipt));
        }
        assert!(Receipt::decode(&[0; 24]).is_none());
        let mut invalid = [0; 25];
        invalid[24] = 3;
        assert!(Receipt::decode(&invalid).is_none());
    }
}
