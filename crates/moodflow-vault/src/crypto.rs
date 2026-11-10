use crate::{Error, Result};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const MAX_SEALED_BYTES: usize = 8 * 1024 * 1024;

/// Deliberately has no Debug implementation.
pub struct Key(Zeroizing<[u8; 32]>);
impl Key {
    pub fn generate() -> Result<Self> {
        let mut bytes = Zeroizing::new([0; 32]);
        getrandom::fill(bytes.as_mut()).map_err(|_| Error::Random)?;
        Ok(Self(bytes))
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut key = Zeroizing::new([0; 32]);
        if bytes.len() != key.len() {
            return Err(Error::Invalid);
        }
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }
    fn cipher(&self) -> Result<XChaCha20Poly1305> {
        XChaCha20Poly1305::new_from_slice(self.0.as_ref()).map_err(|_| Error::Invalid)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    nonce: [u8; 24],
    #[serde(
        serialize_with = "ciphertext_serialize",
        deserialize_with = "ciphertext_deserialize"
    )]
    ciphertext: Vec<u8>,
}

fn ciphertext_serialize<S: serde::Serializer>(
    bytes: &[u8],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    use base64::Engine;
    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}
fn ciphertext_deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<u8>, D::Error> {
    use base64::Engine;
    let text = String::deserialize(deserializer)?;
    if text.len() > (MAX_SEALED_BYTES + 16) * 4 / 3 + 4 {
        return Err(serde::de::Error::custom("vault_limit"));
    }
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| serde::de::Error::custom("invalid_ciphertext"))
}

pub fn seal(key: &Key, context: &[u8], plain: &[u8]) -> Result<Sealed> {
    if plain.len() > MAX_SEALED_BYTES || context.len() > 1024 {
        return Err(Error::Limit);
    }
    let mut nonce = [0; 24];
    getrandom::fill(&mut nonce).map_err(|_| Error::Random)?;
    let ciphertext = key
        .cipher()?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plain,
                aad: context,
            },
        )
        .map_err(|_| Error::Crypt)?;
    Ok(Sealed { nonce, ciphertext })
}
pub fn open(key: &Key, context: &[u8], sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>> {
    if sealed.ciphertext.len() > MAX_SEALED_BYTES + 16 || context.len() > 1024 {
        return Err(Error::Limit);
    }
    key.cipher()?
        .decrypt(
            &XNonce::from(sealed.nonce),
            Payload {
                msg: &sealed.ciphertext,
                aad: context,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::Crypt)
}

/// This record must be protected by an OS credential protector before persistence.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyWrap {
    version: u8,
    salt: [u8; 16],
    sealed: Sealed,
}
fn password_key(password: &str, salt: &[u8; 16]) -> Result<Key> {
    if password.len() > 1024 || password.is_empty() {
        return Err(Error::Invalid);
    }
    // KeyWrap version 1 freezes these parameters. Changing them requires a new
    // format version and an explicit migration, or existing keys become unreadable.
    let params = argon2::Params::new(65536, 3, 1, Some(32)).map_err(|_| Error::Invalid)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = Zeroizing::new([0; 32]);
    let mut memory = Zeroizing::new(vec![argon2::Block::default(); 65536]);
    argon
        .hash_password_into_with_memory(password.as_bytes(), salt, key.as_mut(), &mut memory[..])
        .map_err(|_| Error::Crypt)?;
    Ok(Key(key))
}
impl KeyWrap {
    pub fn create(key: &Key, password: &str, vault_id: &str) -> Result<Self> {
        if password.chars().count() < 12 {
            return Err(Error::WeakPassword);
        }
        let mut salt = [0; 16];
        getrandom::fill(&mut salt).map_err(|_| Error::Random)?;
        let wrapping = password_key(password, &salt)?;
        Ok(Self {
            version: 1,
            salt,
            sealed: seal(
                &wrapping,
                format!("Moodflow vault key v1:{vault_id}").as_bytes(),
                key.0.as_ref(),
            )?,
        })
    }
    pub fn unlock(&self, password: &str, vault_id: &str) -> Result<Key> {
        if self.version != 1 {
            return Err(Error::Invalid);
        }
        let wrapping = password_key(password, &self.salt)?;
        Key::from_bytes(&open(
            &wrapping,
            format!("Moodflow vault key v1:{vault_id}").as_bytes(),
            &self.sealed,
        )?)
    }
}

pub fn generate_password(length: usize, symbols: bool) -> Result<Zeroizing<String>> {
    if !(16..=128).contains(&length) {
        return Err(Error::Invalid);
    }
    let mut groups: Vec<&[u8]> = vec![
        b"ABCDEFGHJKLMNPQRSTUVWXYZ",
        b"abcdefghijkmnopqrstuvwxyz",
        b"23456789",
    ];
    if symbols {
        groups.push(b"!@#$%&*+-=?_");
    }
    let alphabet = groups.concat();
    let mut out = Zeroizing::new(Vec::with_capacity(length));
    for group in groups {
        out.push(group[random_index(group.len())?]);
    }
    while out.len() < length {
        out.push(alphabet[random_index(alphabet.len())?]);
    }
    for i in (1..out.len()).rev() {
        let j = random_index(i + 1)?;
        out.swap(i, j);
    }
    let string = std::str::from_utf8(&out)
        .map_err(|_| Error::Invalid)?
        .to_owned();
    Ok(Zeroizing::new(string))
}
fn random_index(length: usize) -> Result<usize> {
    let cutoff = 256 - (256 % length);
    loop {
        let mut byte = [0];
        getrandom::fill(&mut byte).map_err(|_| Error::Random)?;
        if (byte[0] as usize) < cutoff {
            return Ok(byte[0] as usize % length);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_ciphertext_binds_vault_identity_and_rejects_tampering() {
        let key = Key::generate().unwrap();
        let mut sealed = seal(&key, b"vault-a", b"sample credential").unwrap();
        assert_eq!(
            open(&key, b"vault-a", &sealed).unwrap().as_slice(),
            b"sample credential"
        );
        assert!(open(&key, b"vault-b", &sealed).is_err());
        assert!(open(&Key::generate().unwrap(), b"vault-a", &sealed).is_err());
        let other = seal(&key, b"vault-a", b"sample credential").unwrap();
        assert_ne!(sealed.nonce, other.nonce);
        sealed.ciphertext[0] ^= 1;
        assert!(open(&key, b"vault-a", &sealed).is_err());
    }
    #[test]
    fn passphrase_wrap_requires_the_real_secret_and_new_random_salt() {
        let key = Key::generate().unwrap();
        let wrap = KeyWrap::create(&key, "correct horse example battery", "id").unwrap();
        assert!(wrap.unlock("incorrect password", "id").is_err());
        assert!(
            wrap.unlock("correct horse example battery", "another-id")
                .is_err()
        );
        let restored = wrap.unlock("correct horse example battery", "id").unwrap();
        let data = seal(&key, b"test", b"ok").unwrap();
        assert_eq!(open(&restored, b"test", &data).unwrap().as_slice(), b"ok");
        assert!(KeyWrap::create(&key, "short", "id").is_err());
    }
    #[test]
    fn password_generator_uses_requested_length_and_categories() {
        let first = generate_password(24, true).unwrap();
        assert_eq!(first.len(), 24);
        assert!(first.bytes().any(|b| b.is_ascii_uppercase()));
        assert!(first.bytes().any(|b| b.is_ascii_lowercase()));
        assert!(first.bytes().any(|b| b.is_ascii_digit()));
        assert!(first.bytes().any(|b| !b.is_ascii_alphanumeric()));
        assert_ne!(*first, *generate_password(24, true).unwrap());
        assert!(generate_password(15, false).is_err());
    }
}
