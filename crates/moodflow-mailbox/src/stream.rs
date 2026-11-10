use super::*;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// These are the only external operations. No read/search/execute method exists.
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WriteRequest {
    Push {
        session_id: String,
        message_id: String,
        text: String,
    },
    Begin {
        session_id: String,
        stream_id: String,
    },
    Chunk {
        stream_id: String,
        sequence: u32,
        text: String,
    },
    Finish {
        session_id: String,
        stream_id: String,
    },
    Cancel {
        stream_id: String,
    },
}
struct Stream {
    session: String,
    body: String,
    sequence: u32,
    started: Instant,
}
#[derive(Default)]
pub struct Streams {
    pending: BTreeMap<String, Stream>,
}
impl Streams {
    pub fn clear(&mut self) {
        self.pending.clear();
    }
    pub fn cancel(&mut self, id: &str) {
        self.pending.remove(id);
    }
    pub fn contains(&self, id: &str) -> bool {
        self.pending.contains_key(id)
    }
    pub fn begin(&mut self, id: &str, session: &str) -> Result<()> {
        self.pending
            .retain(|_, s| s.started.elapsed() < Duration::from_secs(120));
        valid_id(id)?;
        valid_id(session)?;
        ensure!(
            self.pending.len() < 16 && !self.pending.contains_key(id),
            "workspace.limit"
        );
        self.pending.insert(
            id.into(),
            Stream {
                session: session.into(),
                body: String::new(),
                sequence: 0,
                started: Instant::now(),
            },
        );
        Ok(())
    }
    pub fn chunk(&mut self, id: &str, sequence: u32, part: &str) -> Result<()> {
        let result = (|| {
            let stream = self
                .pending
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("workspace.missing"))?;
            ensure!(
                stream.started.elapsed() < Duration::from_secs(120)
                    && stream.sequence == sequence
                    && sequence < 16384,
                "workspace.invalid_stream"
            );
            ensure!(
                stream.body.len().saturating_add(part.len()) <= MAX_TEXT_UNITS * 4,
                "workspace.limit"
            );
            text(part, MAX_TEXT_UNITS)?;
            ensure!(
                stream.body.encode_utf16().count() + part.encode_utf16().count() <= MAX_TEXT_UNITS,
                "workspace.limit"
            );
            stream.body.push_str(part);
            stream.sequence += 1;
            Ok(())
        })();
        if result.is_err() {
            self.pending.remove(id);
        }
        result
    }
    /// Prepare a complete message. Cancel only after the caller atomically saves it.
    pub fn finish(&mut self, id: &str, now: u64) -> Result<(String, Message)> {
        let stream = self
            .pending
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("workspace.missing"))?;
        ensure!(
            stream.started.elapsed() < Duration::from_secs(120),
            "workspace.invalid_stream"
        );
        let message = Message {
            id: id.into(),
            role: Role::External,
            body: stream.body.clone(),
            created_at: now,
            reply_to: None,
            mock: false,
            files: vec![],
        };
        message.validate()?;
        Ok((stream.session.clone(), message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unfinished_failed_and_cancelled_streams_never_become_messages() {
        let session = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let mut streams = Streams::default();
        streams.begin(&id, &session).unwrap();
        streams.chunk(&id, 0, "合成").unwrap();
        assert!(streams.chunk(&id, 2, "missing sequence").is_err());
        assert!(streams.finish(&id, 1).is_err());
        streams.begin(&id, &session).unwrap();
        streams.chunk(&id, 0, "complete ").unwrap();
        streams.chunk(&id, 1, "message").unwrap();
        let (target, message) = streams.finish(&id, 2).unwrap();
        assert_eq!(target, session);
        assert_eq!(message.body, "complete message");
        streams.cancel(&id);
        streams.begin(&id, &session).unwrap();
        assert!(
            streams
                .chunk(&id, 0, &"x".repeat(MAX_TEXT_UNITS + 1))
                .is_err()
        );
        assert!(streams.finish(&id, 2).is_err());
        assert!(
            serde_json::from_str::<WriteRequest>(r#"{"operation":"read","session_id":"anything"}"#)
                .is_err()
        );
    }
}
