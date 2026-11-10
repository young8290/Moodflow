//! A short, monotonic unlock lease; the host must sweep it even when the UI is idle.
use crate::{Error, Key, Result};
use std::time::{Duration, Instant};

pub struct Session {
    key: Option<Key>,
    id: String,
    expires: Instant,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            key: None,
            id: String::new(),
            expires: Instant::now(),
        }
    }
}
impl Session {
    pub fn unlock(&mut self, id: String, key: Key) {
        self.lock();
        self.id = id;
        self.key = Some(key);
        self.expires = Instant::now() + Duration::from_secs(60);
    }
    pub fn lock(&mut self) {
        self.key = None;
        self.id.clear();
    }
    fn check_at(&mut self, allowed: bool, now: Instant) -> bool {
        if !allowed || now >= self.expires {
            self.lock();
        }
        self.key.is_some()
    }
    pub fn check(&mut self, allowed: bool) -> bool {
        self.check_at(allowed, Instant::now())
    }
    pub fn key(&mut self, allowed: bool, id: &str) -> Result<&Key> {
        if !self.check(allowed) || self.id != id {
            self.lock();
            return Err(Error::Locked);
        }
        self.key.as_ref().ok_or(Error::Locked)
    }
    pub fn remaining_ms(&mut self, allowed: bool) -> u64 {
        if !self.check(allowed) {
            return 0;
        }
        self.expires
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_expiry_and_privacy_revoke_the_real_key_without_extending_the_lease() {
        let mut session = Session::default();
        session.unlock("first".into(), Key::generate().unwrap());
        let expires = session.expires;
        assert!(session.key(true, "first").is_ok());
        assert_eq!(session.expires, expires);
        assert!(!session.check_at(true, expires));
        assert!(session.key.is_none());
        session.unlock("first".into(), Key::generate().unwrap());
        assert!(!session.check(false));
        session.unlock("first".into(), Key::generate().unwrap());
        assert!(session.key(true, "another").is_err());
        assert!(session.key.is_none());
    }
}
