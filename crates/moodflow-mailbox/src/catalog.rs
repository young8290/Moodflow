//! Local search, lifecycle changes and inert transcript export.
//! This module is never exposed through the external write-only inbox protocol.
use crate::{Kind, MAX_SESSIONS, MAX_TOTAL_BYTES, Session, Store, valid_id};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFilter {
    #[default]
    Active,
    Archived,
    All,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Title,
    Messages,
    #[default]
    All,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Updated,
    Created,
    Title,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub kind: Option<Kind>,
    pub archive: ArchiveFilter,
    pub scope: SearchScope,
    pub unread_only: bool,
    pub sort: Sort,
    pub offset: usize,
    pub limit: usize,
}
impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            kind: None,
            archive: ArchiveFilter::Active,
            scope: SearchScope::All,
            unread_only: false,
            sort: Sort::Updated,
            offset: 0,
            limit: 20,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct MessageHit {
    pub id: String,
    pub role: crate::Role,
    pub snippet: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub archived: bool,
    pub unread: bool,
    pub count: usize,
    pub revision: String,
    pub title_hit: bool,
    pub matched_messages: usize,
    pub matches: Vec<MessageHit>,
}
#[derive(Debug, Serialize)]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub unreadable: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub revision: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Archive { value: bool },
    Unread { value: bool },
}
#[derive(Debug, Serialize)]
pub struct ChangeResult {
    pub id: String,
    pub status: String,
    pub revision: Option<String>,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptFormat {
    Markdown,
    Json,
}

fn snippet(text: &str, term: Option<&String>) -> String {
    let lower = text.to_lowercase();
    let position = term.and_then(|t| lower.find(t)).unwrap_or(0);
    let start = lower[..position].chars().count().saturating_sub(40);
    let chars = text.chars().collect::<Vec<_>>();
    let start = start.min(chars.len());
    let end = (start + 180).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}
fn message_text(message: &crate::Message) -> String {
    let mut text = message.body.clone();
    for file in &message.files {
        text.push('\n');
        text.push_str(&file.name);
    }
    text
}
impl Store {
    /// Damaged records are isolated by ID. Tombstones and unrelated files are skipped.
    fn scan_catalog(&self) -> Result<(Vec<Session>, Vec<String>)> {
        if !self.root.exists() {
            return Ok((vec![], vec![]));
        }
        let mut sessions = Vec::new();
        let mut unreadable = Vec::new();
        let mut total = 0_u64;
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!(index < 20_000, "workspace.limit");
            let entry = entry?;
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = entry
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
            else {
                continue;
            };
            if valid_id(&id).is_err() || self.root.join(format!("{id}.deleted")).exists() {
                continue;
            }
            ensure!(
                sessions.len() + unreadable.len() < MAX_SESSIONS,
                "workspace.limit"
            );
            // Count even damaged files against the read budget; never follow links.
            let meta = fs::symlink_metadata(entry.path())?;
            total = total.saturating_add(meta.len());
            ensure!(total <= MAX_TOTAL_BYTES, "workspace.limit");
            match self.get(&id) {
                Ok(session) => sessions.push(session),
                Err(_) => unreadable.push(id),
            }
        }
        unreadable.sort();
        Ok((sessions, unreadable))
    }
    pub fn available_sessions(&self) -> Result<(Vec<Session>, Vec<String>)> {
        if !self.root.exists() {
            return Ok((vec![], vec![]));
        }
        let _lock = self.lock()?;
        let (mut sessions, unreadable) = self.scan_catalog()?;
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok((sessions, unreadable))
    }
    pub fn search(&self, request: &SearchRequest) -> Result<SearchPage> {
        ensure!(
            request.query.encode_utf16().count() <= 256
                && !request.query.contains('\0')
                && request.limit > 0
                && request.limit <= 50
                && request.offset <= MAX_SESSIONS,
            "workspace.limit"
        );
        let terms = request
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        ensure!(terms.len() <= 8, "workspace.limit");
        if !self.root.exists() {
            return Ok(SearchPage {
                items: vec![],
                total: 0,
                offset: request.offset,
                limit: request.limit,
                unreadable: vec![],
            });
        }
        let _lock = self.lock()?;
        let (sessions, unreadable) = self.scan_catalog()?;
        let mut items = Vec::new();
        for session in sessions {
            if request.kind.is_some_and(|kind| kind != session.kind)
                || (request.unread_only && !session.unread)
            {
                continue;
            }
            if matches!(request.archive, ArchiveFilter::Active) && session.archived
                || matches!(request.archive, ArchiveFilter::Archived) && !session.archived
            {
                continue;
            }
            let title = session.title.to_lowercase();
            let bodies = session
                .messages
                .iter()
                .map(message_text)
                .collect::<Vec<_>>();
            let lower_bodies = bodies.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
            let all_match = terms.iter().all(|term| match request.scope {
                SearchScope::Title => title.contains(term),
                SearchScope::Messages => lower_bodies.iter().any(|body| body.contains(term)),
                SearchScope::All => {
                    title.contains(term) || lower_bodies.iter().any(|body| body.contains(term))
                }
            });
            if !all_match {
                continue;
            }
            let mut matches = Vec::new();
            let mut matched_messages = 0;
            if !matches!(request.scope, SearchScope::Title) && !terms.is_empty() {
                for (i, body) in lower_bodies.iter().enumerate() {
                    if let Some(term) = terms.iter().find(|t| body.contains(t.as_str())) {
                        matched_messages += 1;
                        if matches.len() < 3 {
                            matches.push(MessageHit {
                                id: session.messages[i].id.clone(),
                                role: session.messages[i].role,
                                snippet: snippet(&bodies[i], Some(term)),
                            });
                        }
                    }
                }
            }
            items.push(SearchHit {
                id: session.id,
                kind: session.kind,
                title: session.title,
                created_at: session.created_at,
                updated_at: session.updated_at,
                archived: session.archived,
                unread: session.unread,
                count: session.messages.len(),
                revision: session.revision,
                title_hit: !terms.is_empty() && terms.iter().any(|t| title.contains(t)),
                matched_messages,
                matches,
            });
        }
        items.sort_by(|a, b| {
            match request.sort {
                Sort::Updated => b.updated_at.cmp(&a.updated_at),
                Sort::Created => b.created_at.cmp(&a.created_at),
                Sort::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            }
            .then(a.id.cmp(&b.id))
        });
        let total = items.len();
        let items = items
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .collect();
        Ok(SearchPage {
            items,
            total,
            offset: request.offset,
            limit: request.limit,
            unreadable,
        })
    }
    /// Each selected row is independently revision checked. No content or model changes.
    pub fn change_sessions(
        &self,
        selection: &[Selection],
        change: &Change,
        now: u64,
    ) -> Result<Vec<ChangeResult>> {
        ensure!(
            !selection.is_empty() && selection.len() <= 50,
            "workspace.limit"
        );
        let mut ids = BTreeSet::new();
        for item in selection {
            valid_id(&item.id)?;
            ensure!(
                ids.insert(&item.id) && item.revision.len() == 64,
                "workspace.invalid"
            );
        }
        let mut outcomes = Vec::new();
        for item in selection {
            let result = (|| -> Result<Session> {
                let mut session = self.get(&item.id)?;
                ensure!(session.revision == item.revision, "workspace.conflict");
                match change {
                    Change::Archive { value } => session.archived = *value,
                    Change::Unread { value } => session.unread = *value,
                }
                session.updated_at = now;
                self.save(&session, Some(&item.revision))
            })();
            let (status, revision) = match result {
                Ok(saved) => ("updated", Some(saved.revision)),
                Err(e) if e.to_string() == "workspace.conflict" => ("conflict", None),
                Err(_) => ("failed", None),
            };
            outcomes.push(ChangeResult {
                id: item.id.clone(),
                status: status.into(),
                revision,
            });
        }
        Ok(outcomes)
    }
    pub fn rename_session(&self, selection: &Selection, title: &str, now: u64) -> Result<Session> {
        let mut session = self.get(&selection.id)?;
        ensure!(session.revision == selection.revision, "workspace.conflict");
        session.title = title.trim().into();
        session.updated_at = now;
        self.save(&session, Some(&selection.revision))
    }
    /// Plain inert data only: no terminal output capture, file reads or AI requests.
    pub fn transcript(
        &self,
        selection: &Selection,
        format: TranscriptFormat,
        include_instructions: bool,
    ) -> Result<Vec<u8>> {
        let _lock = self.lock()?;
        let mut session = self.get(&selection.id)?;
        ensure!(session.revision == selection.revision, "workspace.conflict");
        if !include_instructions {
            session.instructions.clear();
        }
        let bytes = match format {
            TranscriptFormat::Json => serde_json::to_vec_pretty(
                &serde_json::json!({"format":"moodflow-transcript","version":1,"session":session}),
            )?,
            TranscriptFormat::Markdown => {
                let mut output = format!(
                    "# {}\n\nKind: {:?}\nSession: {}\n\n",
                    markdown_title(&session.title),
                    session.kind,
                    session.id
                );
                if include_instructions && !session.instructions.is_empty() {
                    output.push_str("## Instructions\n\n");
                    fenced(&mut output, &session.instructions);
                }
                for message in &session.messages {
                    output.push_str(&format!(
                        "## {:?} · {}\n\n",
                        message.role, message.created_at
                    ));
                    fenced(&mut output, &message_text(message));
                }
                output.into_bytes()
            }
        };
        ensure!(bytes.len() <= 16 * 1024 * 1024, "workspace.limit");
        Ok(bytes)
    }
}
fn fenced(output: &mut String, body: &str) {
    let longest = body.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    output.push_str(&format!("{fence}text\n{body}\n{fence}\n\n"));
}

fn markdown_title(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        match c {
            '\r' | '\n' => out.push(' '),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if "\\`*_{}[]()#+-.!|".contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}
