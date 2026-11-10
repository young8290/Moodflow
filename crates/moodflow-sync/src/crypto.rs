use crate::*;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub version: u32,
    pub dataset: String,
    pub encrypted: bool,
    salt: [u8; 16],
    check: Vec<u8>,
}
/// Neither keys nor recovery phrases implement Debug or serialization.
pub struct Codec {
    pub dataset: String,
    key: Option<Zeroizing<[u8; 32]>>,
}
impl Header {
    pub fn create(phrase: Option<&str>) -> Result<(Self, Codec)> {
        let mut header = Self {
            version: 1,
            dataset: uuid::Uuid::new_v4().to_string(),
            encrypted: phrase.is_some(),
            salt: [0; 16],
            check: vec![],
        };
        getrandom::fill(&mut header.salt).map_err(|_| anyhow::anyhow!("sync.random"))?;
        let codec = header.key(phrase)?;
        header.check = codec.seal("header", b"Moodflow Sync v1")?;
        Ok((header, codec))
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= 32768, "sync.limit");
        let header: Self =
            serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("sync.invalid"))?;
        ensure!(header.version == 1, "sync.version");
        id(&header.dataset)?;
        ensure!(header.check.len() <= 1024, "sync.limit");
        Ok(header)
    }
    fn key(&self, phrase: Option<&str>) -> Result<Codec> {
        let key = match (self.encrypted, phrase) {
            (true, Some(phrase)) => {
                ensure!(
                    (24..=1024).contains(&phrase.chars().count()),
                    "sync.recovery"
                );
                let params = argon2::Params::new(65536, 3, 1, Some(32))
                    .map_err(|_| anyhow::anyhow!("sync.invalid"))?;
                let mut bytes = Zeroizing::new([0; 32]);
                argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
                    .hash_password_into(phrase.as_bytes(), &self.salt, bytes.as_mut())
                    .map_err(|_| anyhow::anyhow!("sync.recovery"))?;
                Some(bytes)
            }
            (false, None) => None,
            _ => anyhow::bail!("sync.mode_changed"),
        };
        Ok(Codec {
            dataset: self.dataset.clone(),
            key,
        })
    }
    pub fn unlock(&self, phrase: Option<&str>) -> Result<Codec> {
        let codec = self.key(phrase)?;
        ensure!(
            codec.open("header", &self.check)?.as_slice() == b"Moodflow Sync v1",
            "sync.recovery"
        );
        Ok(codec)
    }
}
impl Codec {
    pub fn encrypted(&self) -> bool {
        self.key.is_some()
    }
    pub fn seal(&self, purpose: &str, plain: &[u8]) -> Result<Vec<u8>> {
        ensure!(
            plain.len() <= MAX_BLOB && purpose.len() <= 256,
            "sync.limit"
        );
        if let Some(key) = &self.key {
            let mut nonce = [0; 24];
            getrandom::fill(&mut nonce).map_err(|_| anyhow::anyhow!("sync.random"))?;
            let aad = format!("moodflow-sync/v1/{}/{}", self.dataset, purpose);
            let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
                .map_err(|_| anyhow::anyhow!("sync.invalid"))?;
            let sealed = cipher
                .encrypt(
                    &XNonce::from(nonce),
                    Payload {
                        msg: plain,
                        aad: aad.as_bytes(),
                    },
                )
                .map_err(|_| anyhow::anyhow!("sync.crypto"))?;
            let mut out = b"MFSC1E".to_vec();
            out.extend_from_slice(&nonce);
            out.extend_from_slice(&sealed);
            Ok(out)
        } else {
            let mut out = b"MFSC1P".to_vec();
            out.extend_from_slice(plain);
            Ok(out)
        }
    }
    pub fn open(&self, purpose: &str, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(
            bytes.len() <= MAX_BLOB + 64 && purpose.len() <= 256,
            "sync.limit"
        );
        if let Some(key) = &self.key {
            ensure!(
                bytes.len() >= 46 && bytes.starts_with(b"MFSC1E"),
                "sync.crypto"
            );
            let aad = format!("moodflow-sync/v1/{}/{}", self.dataset, purpose);
            let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
                .map_err(|_| anyhow::anyhow!("sync.invalid"))?;
            let nonce: [u8; 24] = bytes[6..30]
                .try_into()
                .map_err(|_| anyhow::anyhow!("sync.crypto"))?;
            cipher
                .decrypt(
                    &XNonce::from(nonce),
                    Payload {
                        msg: &bytes[30..],
                        aad: aad.as_bytes(),
                    },
                )
                .map(Zeroizing::new)
                .map_err(|_| anyhow::anyhow!("sync.recovery"))
        } else {
            ensure!(bytes.starts_with(b"MFSC1P"), "sync.mode_changed");
            Ok(Zeroizing::new(bytes[6..].to_vec()))
        }
    }
}
