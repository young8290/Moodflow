//! Independent workspace sessions. External content is inert data, never an input or command.
mod inbox;
pub mod protocol;
mod store;
mod stream;
use anyhow::{Result, ensure};
pub use inbox::Inbox;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub use store::Store;
pub use stream::{Streams, WriteRequest};

pub const MAX_SESSIONS: usize = 200;
pub const MAX_MESSAGES: usize = 200;
pub const MAX_TEXT_UNITS: usize = 16384;
pub const MAX_SESSION_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

pub fn valid_id(id: &str) -> Result<()> {
    ensure!(
        uuid::Uuid::parse_str(id)?.hyphenated().to_string() == id,
        "workspace.invalid"
    );
    Ok(())
}
fn text(text: &str, maximum: usize) -> Result<()> {
    ensure!(
        text.encode_utf16().count() <= maximum && !text.contains('\0'),
        "workspace.limit"
    );
    Ok(())
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Chat,
    Inbox,
    Terminal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    External,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenModel {
    pub endpoint: String,
    pub model: String,
}
impl FrozenModel {
    pub fn validate(&self) -> Result<()> {
        text(&self.endpoint, 2048)?;
        text(&self.model, 256)?;
        ensure!(
            !self.endpoint.is_empty() && !self.model.is_empty(),
            "workspace.model_changed"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
impl Attachment {
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        text(&self.name, 256)?;
        ensure!(
            !self.name.is_empty()
                && !self.name.contains(['/', '\\'])
                && self.bytes <= MAX_FILE_BYTES
                && self.sha256.len() == 64
                && self.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "workspace.invalid"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub role: Role,
    pub body: String,
    pub created_at: u64,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub mock: bool,
    #[serde(default)]
    pub files: Vec<Attachment>,
}
impl Message {
    pub fn new(role: Role, body: String, now: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            body,
            created_at: now,
            reply_to: None,
            mock: false,
            files: vec![],
        }
    }
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        text(&self.body, MAX_TEXT_UNITS)?;
        if let Some(id) = &self.reply_to {
            valid_id(id)?;
            ensure!(self.role == Role::Assistant, "workspace.invalid");
        }
        ensure!(
            !self.body.trim().is_empty() || !self.files.is_empty(),
            "workspace.empty"
        );
        ensure!(self.files.len() <= 16, "workspace.limit");
        let mut ids = BTreeSet::new();
        for file in &self.files {
            file.validate()?;
            ensure!(ids.insert(&file.id), "workspace.invalid");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub version: u8,
    pub id: String,
    pub kind: Kind,
    pub title: String,
    #[serde(default)]
    pub instructions: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub archived: bool,
    pub unread: bool,
    pub model: Option<FrozenModel>,
    pub messages: Vec<Message>,
    #[serde(skip)]
    pub revision: String,
}
impl Session {
    pub fn new(kind: Kind, title: String, now: u64) -> Self {
        Self {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            title,
            instructions: String::new(),
            created_at: now,
            updated_at: now,
            archived: false,
            unread: false,
            model: None,
            messages: vec![],
            revision: String::new(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        valid_id(&self.id)?;
        text(&self.title, 256)?;
        text(&self.instructions, 8192)?;
        ensure!(
            self.version == 1
                && !self.title.trim().is_empty()
                && self.messages.len() <= MAX_MESSAGES,
            "workspace.limit"
        );
        if let Some(model) = &self.model {
            ensure!(self.kind == Kind::Chat, "workspace.invalid");
            model.validate()?;
        }
        let mut ids = BTreeSet::new();
        let mut users = BTreeSet::new();
        for message in &self.messages {
            message.validate()?;
            if message.role == Role::User {
                users.insert(&message.id);
            }
            if message.role == Role::Assistant {
                ensure!(
                    message
                        .reply_to
                        .as_ref()
                        .is_some_and(|id| users.contains(id)),
                    "workspace.invalid"
                );
            }
            ensure!(ids.insert(&message.id), "workspace.invalid");
            match self.kind {
                Kind::Chat => ensure!(
                    matches!(message.role, Role::User | Role::Assistant | Role::Note),
                    "workspace.invalid"
                ),
                Kind::Inbox => ensure!(
                    matches!(message.role, Role::External | Role::Note),
                    "workspace.invalid"
                ),
                Kind::Terminal => ensure!(message.role == Role::Note, "workspace.invalid"),
            }
        }
        Ok(())
    }
    /// Whole batches commit together; an over-limit or duplicate batch leaves self unchanged.
    pub fn append(
        &mut self,
        messages: Vec<Message>,
        model: Option<FrozenModel>,
        now: u64,
    ) -> Result<()> {
        let mut next = self.clone();
        if let Some(model) = model {
            if let Some(frozen) = &next.model {
                ensure!(frozen == &model, "workspace.model_changed");
            }
            next.model = Some(model);
        }
        next.unread |= messages.iter().any(|m| m.role == Role::External);
        next.messages.extend(messages);
        next.updated_at = now;
        next.validate()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn batches_are_atomic_and_model_identity_never_changes_implicitly() {
        let mut session = Session::new(Kind::Chat, "fixture".into(), 1);
        let frozen = FrozenModel {
            endpoint: "https://example.invalid/v1".into(),
            model: "pinned-model".into(),
        };
        session
            .append(
                vec![Message::new(Role::User, "hello".into(), 1)],
                Some(frozen.clone()),
                1,
            )
            .unwrap();
        let other = FrozenModel {
            model: "other-model".into(),
            ..frozen
        };
        assert!(
            session
                .append(
                    vec![Message::new(Role::User, "changed".into(), 2)],
                    Some(other),
                    2
                )
                .is_err()
        );
        assert_eq!(session.messages.len(), 1);
        assert!(
            session
                .append(
                    (0..200)
                        .map(|_| Message::new(Role::User, "bounded".into(), 2))
                        .collect(),
                    None,
                    2
                )
                .is_err()
        );
        assert_eq!(session.messages.len(), 1);
        assert!(
            session
                .append(
                    vec![Message::new(Role::User, "𠮷".repeat(8193), 2)],
                    None,
                    2
                )
                .is_err()
        );
        let mut terminal = Session::new(Kind::Terminal, "terminal fixture".into(), 1);
        assert!(
            terminal
                .append(
                    vec![Message::new(Role::Assistant, "not a chat".into(), 1)],
                    None,
                    1
                )
                .is_err()
        );
    }
}
