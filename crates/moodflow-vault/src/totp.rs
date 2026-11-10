use crate::{Error, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct Totp {
    pub secret: String,
    #[zeroize(skip)]
    pub algorithm: Algorithm,
    pub digits: u8,
    pub period: u32,
}
impl Totp {
    pub fn validate(&self) -> Result<()> {
        if ![6, 8].contains(&self.digits) || !(15..=120).contains(&self.period) {
            return Err(Error::Invalid);
        }
        decode(&self.secret).map(|_| ())
    }
    pub fn code(&self, now_secs: u64) -> Result<Zeroizing<String>> {
        self.validate()?;
        let secret = decode(&self.secret)?;
        let counter = (now_secs / self.period as u64).to_be_bytes();
        let digest = match self.algorithm {
            Algorithm::Sha1 => {
                let mut h = Hmac::<Sha1>::new_from_slice(&secret).map_err(|_| Error::Invalid)?;
                h.update(&counter);
                Zeroizing::new(h.finalize().into_bytes().to_vec())
            }
            Algorithm::Sha256 => {
                let mut h = Hmac::<Sha256>::new_from_slice(&secret).map_err(|_| Error::Invalid)?;
                h.update(&counter);
                Zeroizing::new(h.finalize().into_bytes().to_vec())
            }
            Algorithm::Sha512 => {
                let mut h = Hmac::<Sha512>::new_from_slice(&secret).map_err(|_| Error::Invalid)?;
                h.update(&counter);
                Zeroizing::new(h.finalize().into_bytes().to_vec())
            }
        };
        let offset = (digest[digest.len() - 1] & 15) as usize;
        let binary = u32::from_be_bytes(
            digest[offset..offset + 4]
                .try_into()
                .map_err(|_| Error::Invalid)?,
        ) & 0x7fffffff;
        Ok(Zeroizing::new(format!(
            "{:0width$}",
            binary % 10_u32.pow(self.digits as u32),
            width = self.digits as usize
        )))
    }
    pub fn from_uri(input: &str) -> Result<Self> {
        if input.len() > 2048 {
            return Err(Error::Limit);
        }
        let uri = url::Url::parse(input).map_err(|_| Error::Invalid)?;
        if uri.scheme() != "otpauth"
            || uri.host_str() != Some("totp")
            || !uri.username().is_empty()
            || uri.password().is_some()
        {
            return Err(Error::Invalid);
        }
        let mut out = Self {
            secret: String::new(),
            algorithm: Algorithm::Sha1,
            digits: 6,
            period: 30,
        };
        let mut seen = std::collections::BTreeSet::new();
        for (key, value) in uri.query_pairs() {
            if !seen.insert(key.to_string()) {
                return Err(Error::Invalid);
            }
            match key.as_ref() {
                "secret" => out.secret = value.into_owned(),
                "algorithm" => {
                    out.algorithm = match value.to_ascii_uppercase().as_str() {
                        "SHA1" => Algorithm::Sha1,
                        "SHA256" => Algorithm::Sha256,
                        "SHA512" => Algorithm::Sha512,
                        _ => return Err(Error::Invalid),
                    }
                }
                "digits" => out.digits = value.parse().map_err(|_| Error::Invalid)?,
                "period" => out.period = value.parse().map_err(|_| Error::Invalid)?,
                "issuer" => {}
                _ => return Err(Error::Invalid),
            }
        }
        out.validate()?;
        Ok(out)
    }
}
fn decode(input: &str) -> Result<Zeroizing<Vec<u8>>> {
    if input.len() > 256 {
        return Err(Error::Limit);
    }
    let compact = Zeroizing::new(
        input
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect::<String>(),
    );
    let normalized = Zeroizing::new(compact.trim_end_matches('=').to_ascii_uppercase());
    let bytes = data_encoding::BASE32_NOPAD
        .decode(normalized.as_bytes())
        .map(Zeroizing::new)
        .map_err(|_| Error::Invalid)?;
    if !(10..=128).contains(&bytes.len()) {
        return Err(Error::Invalid);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc_6238_all_official_test_vectors() {
        let times = [
            59,
            1111111109,
            1111111111,
            1234567890,
            2000000000,
            20000000000,
        ];
        let vectors = [
            (
                Algorithm::Sha1,
                "12345678901234567890",
                [
                    "94287082", "07081804", "14050471", "89005924", "69279037", "65353130",
                ],
            ),
            (
                Algorithm::Sha256,
                "12345678901234567890123456789012",
                [
                    "46119246", "68084774", "67062674", "91819424", "90698825", "77737706",
                ],
            ),
            (
                Algorithm::Sha512,
                "1234567890123456789012345678901234567890123456789012345678901234",
                [
                    "90693936", "25091201", "99943326", "93441116", "38618901", "47863826",
                ],
            ),
        ];
        for (algorithm, secret, expected) in vectors {
            let totp = Totp {
                secret: data_encoding::BASE32_NOPAD.encode(secret.as_bytes()),
                algorithm,
                digits: 8,
                period: 30,
            };
            for (time, code) in times.into_iter().zip(expected) {
                assert_eq!(totp.code(time).unwrap().as_str(), code);
            }
        }
    }
    #[test]
    fn otpauth_is_totp_only_and_rejects_unsupported_parameters() {
        let t = Totp::from_uri("otpauth://totp/Example:user?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&algorithm=SHA1&digits=8&period=30").unwrap();
        assert_eq!(t.code(59).unwrap().as_str(), "94287082");
        for bad in [
            "otpauth://hotp/x?secret=ABC",
            "https://example.invalid/?secret=ABC",
            "otpauth://totp/x?secret=ABC",
            "otpauth://totp/x?secret=ABC&secret=DEF",
        ] {
            assert!(Totp::from_uri(bad).is_err());
        }
    }
}
