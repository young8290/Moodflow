use super::*;
use serde_json::{Value, json};
#[derive(Default)]
pub struct Inbox {
    streams: Streams,
}
impl Inbox {
    pub fn clear(&mut self) {
        self.streams.clear();
    }
    pub fn accept(&mut self, store: &Store, request: WriteRequest, now: u64) -> Result<Value> {
        match request {
            WriteRequest::Push {
                session_id,
                message_id,
                text,
            } => {
                let mut message = Message::new(Role::External, text, now);
                message.id = message_id;
                Self::deliver(store, &session_id, message, now)
            }
            WriteRequest::Begin {
                session_id,
                stream_id,
            } => {
                let session = store.get(&session_id)?;
                ensure!(
                    session.kind == Kind::Inbox && !session.archived,
                    "workspace.inbox_required"
                );
                self.streams.begin(&stream_id, &session_id)?;
                Ok(json!({"accepted":true}))
            }
            WriteRequest::Chunk {
                stream_id,
                sequence,
                text,
            } => {
                self.streams.chunk(&stream_id, sequence, &text)?;
                Ok(json!({"accepted":true}))
            }
            WriteRequest::Finish {
                session_id,
                stream_id,
            } => {
                valid_id(&session_id)?;
                valid_id(&stream_id)?;
                // An acknowledgment may be lost after the atomic save, including a Hub restart.
                // A completed message can only return its receipt; no content leaves the inbox.
                if !self.streams.contains(&stream_id) {
                    let session = store.get(&session_id)?;
                    ensure!(
                        session.kind == Kind::Inbox && !session.archived,
                        "workspace.inbox_required"
                    );
                    ensure!(
                        session
                            .messages
                            .iter()
                            .any(|m| m.id == stream_id && m.role == Role::External),
                        "workspace.missing"
                    );
                    return Ok(json!({"accepted":true,"message_id":stream_id}));
                }
                let (id, message) = self.streams.finish(&stream_id, now)?;
                ensure!(id == session_id, "workspace.invalid_stream");
                let receipt = Self::deliver(store, &id, message, now)?;
                self.streams.cancel(&stream_id);
                Ok(receipt)
            }
            WriteRequest::Cancel { stream_id } => {
                self.streams.cancel(&stream_id);
                Ok(json!({"accepted":true}))
            }
        }
    }
    fn deliver(store: &Store, id: &str, message: Message, now: u64) -> Result<Value> {
        message.validate()?;
        let mut session = store.get(id)?;
        ensure!(
            session.kind == Kind::Inbox && !session.archived,
            "workspace.inbox_required"
        );
        if let Some(existing) = session.messages.iter().find(|m| m.id == message.id) {
            ensure!(
                existing.role == Role::External
                    && existing.body == message.body
                    && existing.files == message.files,
                "workspace.conflict"
            );
            return Ok(json!({"accepted":true,"message_id":message.id}));
        }
        let revision = session.revision.clone();
        let receipt = message.id.clone();
        session.append(vec![message], None, now)?;
        store.save(&session, Some(&revision))?;
        Ok(json!({"accepted":true,"message_id":receipt}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_writes_only_reach_explicit_inboxes_and_acknowledgments_never_echo_content() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path().join("workspace"));
        let inbox = store
            .save(&Session::new(Kind::Inbox, "inbox fixture".into(), 1), None)
            .unwrap();
        let chat = store
            .save(&Session::new(Kind::Chat, "chat fixture".into(), 1), None)
            .unwrap();
        let message = uuid::Uuid::new_v4().to_string();
        let mut gateway = Inbox::default();
        let push = |id: &str, body: &str| WriteRequest::Push {
            session_id: id.into(),
            message_id: message.clone(),
            text: body.into(),
        };
        assert!(
            gateway
                .accept(&store, push(&chat.id, "external"), 2)
                .is_err()
        );
        let receipt = gateway
            .accept(&store, push(&inbox.id, "private fixture body"), 2)
            .unwrap();
        assert!(!receipt.to_string().contains("private fixture"));
        gateway
            .accept(&store, push(&inbox.id, "private fixture body"), 3)
            .unwrap();
        assert_eq!(store.get(&inbox.id).unwrap().messages.len(), 1);
        assert!(
            gateway
                .accept(&store, push(&inbox.id, "changed"), 3)
                .is_err()
        );
        let stream = uuid::Uuid::new_v4().to_string();
        gateway
            .accept(
                &store,
                WriteRequest::Begin {
                    session_id: inbox.id.clone(),
                    stream_id: stream.clone(),
                },
                3,
            )
            .unwrap();
        gateway
            .accept(
                &store,
                WriteRequest::Chunk {
                    stream_id: stream.clone(),
                    sequence: 0,
                    text: "unfinished".into(),
                },
                3,
            )
            .unwrap();
        assert_eq!(store.get(&inbox.id).unwrap().messages.len(), 1);
        gateway.clear();
        assert!(
            gateway
                .accept(
                    &store,
                    WriteRequest::Finish {
                        session_id: inbox.id.clone(),
                        stream_id: stream
                    },
                    4
                )
                .is_err()
        );
        assert!(store.get(&chat.id).unwrap().messages.is_empty());
    }
    #[test]
    fn completed_stream_receipt_survives_retry_restart_and_wrong_session() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path().join("workspace"));
        let a = store
            .save(&Session::new(Kind::Inbox, "a".into(), 1), None)
            .unwrap();
        let b = store
            .save(&Session::new(Kind::Inbox, "b".into(), 1), None)
            .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let mut inbox = Inbox::default();
        inbox
            .accept(
                &store,
                WriteRequest::Begin {
                    session_id: a.id.clone(),
                    stream_id: id.clone(),
                },
                1,
            )
            .unwrap();
        inbox
            .accept(
                &store,
                WriteRequest::Chunk {
                    stream_id: id.clone(),
                    sequence: 0,
                    text: "complete synthetic content".into(),
                },
                2,
            )
            .unwrap();
        let finish = |session: &str| WriteRequest::Finish {
            session_id: session.into(),
            stream_id: id.clone(),
        };
        assert!(inbox.accept(&store, finish(&b.id), 3).is_err());
        let receipt = inbox.accept(&store, finish(&a.id), 4).unwrap();
        assert!(!receipt.to_string().contains("synthetic"));
        let mut restarted = Inbox::default();
        assert_eq!(restarted.accept(&store, finish(&a.id), 5).unwrap(), receipt);
        assert_eq!(store.get(&a.id).unwrap().messages.len(), 1);
        assert!(store.get(&b.id).unwrap().messages.is_empty());
        assert!(restarted.accept(&store, finish(&b.id), 6).is_err());
    }
}
