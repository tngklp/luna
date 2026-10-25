//! Append-only JSONL transcripts.
//!
//! One JSON object per line. Appending is a single `write` with no rewrite, so a
//! crash mid-turn loses at most the line being written, and a truncated final
//! line is skipped on read instead of failing the whole session.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::error::{LunaError, Result};
use crate::types::{Message, Role};

use super::{Entry, SessionHeader, SessionSummary};

pub struct Transcript {
    path: PathBuf,
    header: SessionHeader,
    messages: Vec<Message>,
    file: File,
    /// Kept in sync on every append so listing never shows stale counts.
    index: Option<super::SessionIndex>,
}

impl Transcript {
    /// Start a new transcript, writing the header line.
    pub fn create(path: &Path, header: &SessionHeader) -> Result<Transcript> {
        Transcript::create_indexed(path, header, None)
    }

    pub fn create_indexed(
        path: &Path,
        header: &SessionHeader,
        index: Option<super::SessionIndex>,
    ) -> Result<Transcript> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
        let line = serde_json::to_string(&Entry::Header(header.clone()))?;
        writeln!(file, "{line}")
            .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
        file.flush().ok();
        let transcript = Transcript {
            path: path.to_path_buf(),
            header: header.clone(),
            messages: Vec::new(),
            file,
            index,
        };
        transcript.refresh_index();
        Ok(transcript)
    }

    /// Reopen an existing transcript for appending.
    pub fn open(path: &Path) -> Result<Transcript> {
        Transcript::open_indexed(path, None)
    }

    pub fn open_indexed(path: &Path, index: Option<super::SessionIndex>) -> Result<Transcript> {
        let file = File::open(path)
            .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
        let reader = BufReader::new(file);
        let mut header: Option<SessionHeader> = None;
        let mut messages = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if line.trim().is_empty() {
                continue;
            }
            // A half-written final line is expected after a crash; skip it.
            let Ok(entry) = serde_json::from_str::<Entry>(&line) else { continue };
            match entry {
                Entry::Header(value) => header = Some(value),
                Entry::Message { message, .. } => messages.push(message),
                Entry::Note { .. } => {}
            }
        }

        let header = header
            .ok_or_else(|| LunaError::Store(format!("{} has no session header", path.display())))?;

        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;

        Ok(Transcript { path: path.to_path_buf(), header, messages, file, index })
    }

    /// Push the current state into the index, if one is attached.
    fn refresh_index(&self) {
        let Some(index) = &self.index else { return };
        if let Ok(summary) = self.summary() {
            if let Err(err) = index.upsert(&summary) {
                tracing::warn!("could not update the session index: {err}");
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn header(&self) -> &SessionHeader {
        &self.header
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.header.name = Some(name.into());
    }

    pub fn append_message(&mut self, message: &Message) -> Result<()> {
        self.write_entry(&Entry::message(message.clone()))?;
        self.messages.push(message.clone());
        if let Some(index) = &self.index {
            let body = message.text();
            if let Err(err) = index.index_message(&self.path.display().to_string(), &body) {
                tracing::warn!("could not index a message: {err}");
            }
        }
        self.refresh_index();
        Ok(())
    }

    pub fn append_note(&mut self, level: &str, text: &str) -> Result<()> {
        self.write_entry(&Entry::note(level, text))
    }

    fn write_entry(&mut self, entry: &Entry) -> Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.file, "{line}")
            .map_err(|source| LunaError::Io { path: self.path.clone(), source })?;
        // Flushing per line is what makes a crash cost one line, not the turn.
        self.file.flush().map_err(|source| LunaError::Io { path: self.path.clone(), source })
    }

    /// The index row for this transcript.
    pub fn summary(&self) -> Result<SessionSummary> {
        let modified = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| {
                chrono::DateTime::<chrono::Utc>::from_timestamp(
                    since.as_secs() as i64,
                    since.subsec_nanos(),
                )
                .map(|time| time.to_rfc3339())
                .unwrap_or_else(|| self.header.created.clone())
            })
            .unwrap_or_else(|| self.header.created.clone());

        let first_message = self
            .messages
            .iter()
            .find(|message| message.role == Role::User)
            .map(|message| message.text())
            .unwrap_or_default();

        Ok(SessionSummary {
            path: self.path.display().to_string(),
            id: self.header.id.clone(),
            cwd: self.header.cwd.clone(),
            name: self.header.name.clone(),
            created: self.header.created.clone(),
            modified,
            message_count: self.messages.len(),
            first_message: first_message.chars().take(200).collect(),
            parent_session_path: self.header.parent.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentBlock;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-jsonl-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("session.jsonl")
    }

    #[test]
    fn appends_and_reopens() {
        let path = temp_path("append");
        let header = SessionHeader::new("s1", "/work", "m");
        let mut transcript = Transcript::create(&path, &header).unwrap();
        transcript.append_message(&Message::user("one")).unwrap();
        transcript.append_message(&Message::assistant(vec![ContentBlock::text("two")])).unwrap();

        let reopened = Transcript::open(&path).unwrap();
        assert_eq!(reopened.messages().len(), 2);
        assert_eq!(reopened.messages()[1].text(), "two");
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_truncated_final_line_is_skipped() {
        let path = temp_path("truncated");
        let header = SessionHeader::new("s1", "/work", "m");
        let mut transcript = Transcript::create(&path, &header).unwrap();
        transcript.append_message(&Message::user("kept")).unwrap();
        drop(transcript);

        // Simulate a crash mid-write.
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{\"kind\":\"message\",\"message\":{{\"role\":\"us").unwrap();
        drop(file);

        let reopened = Transcript::open(&path).unwrap();
        assert_eq!(reopened.messages().len(), 1);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn summary_uses_the_first_user_message_as_a_title() {
        let path = temp_path("summary");
        let header = SessionHeader::new("s1", "/work", "m");
        let mut transcript = Transcript::create(&path, &header).unwrap();
        transcript
            .append_message(&Message::assistant(vec![ContentBlock::text("greeting")]))
            .unwrap();
        transcript.append_message(&Message::user("the real question")).unwrap();

        let summary = transcript.summary().unwrap();
        assert_eq!(summary.first_message, "the real question");
        assert_eq!(summary.message_count, 2);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_file_without_a_header_is_rejected() {
        let path = temp_path("noheader");
        std::fs::write(
            &path,
            "{\"kind\":\"note\",\"level\":\"info\",\"text\":\"x\",\"at\":\"now\"}\n",
        )
        .unwrap();
        assert!(Transcript::open(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
