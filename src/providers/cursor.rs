use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use crate::session::{Agent, Session, none_if_empty};

pub struct Cursor {
    pub db: PathBuf,
    pub projects: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComposerData {
    composer_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    last_updated_at: Option<i64>,
}

impl super::Provider for Cursor {
    fn agent(&self) -> Agent {
        Agent::Cursor
    }

    fn sessions(&self) -> anyhow::Result<Vec<Session>> {
        if !self.db.exists() {
            return Ok(Vec::new());
        }
        let transcripts = self.transcript_dirs()?;
        let mut decoded: HashMap<String, Option<String>> = HashMap::new();
        let conn = Connection::open_with_flags(
            &self.db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("opening {}", self.db.display()))?;
        conn.busy_timeout(Duration::from_secs(1))?;
        let mut statement = conn
            .prepare("SELECT value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
            .context("querying composerData")?;
        let sessions = statement
            .query_map([], |row| row.get::<_, rusqlite::types::Value>(0))
            .context("querying composerData")?
            .filter_map(Result::ok)
            .filter_map(|value| match value {
                rusqlite::types::Value::Text(text) => Some(text.into_bytes()),
                rusqlite::types::Value::Blob(bytes) => Some(bytes),
                _ => None,
            })
            .filter_map(|value| serde_json::from_slice::<ComposerData>(&value).ok())
            .filter_map(|composer| {
                let transcript = transcripts.get(&composer.composer_id);
                let cwd = transcript.map(|(encoded, _)| {
                    decoded
                        .entry(encoded.clone())
                        .or_insert_with(|| decode(encoded))
                        .clone()
                        .unwrap_or_else(|| encoded.clone())
                });
                Some(Session {
                    agent: Agent::Cursor,
                    id: composer.composer_id,
                    title: none_if_empty(composer.name),
                    cwd,
                    branch: None,
                    updated_at: jiff::Timestamp::from_millisecond(
                        composer.last_updated_at.or(composer.created_at).filter(|ms| *ms > 0)?,
                    )
                    .ok()?,
                    path: transcript.map(|(_, path)| path.to_string_lossy().into_owned()),
                })
            })
            .collect();
        Ok(sessions)
    }
}

impl Cursor {
    fn transcript_dirs(&self) -> anyhow::Result<HashMap<String, (String, PathBuf)>> {
        let mut map = HashMap::new();
        if !self.projects.exists() {
            return Ok(map);
        }
        for project in fs::read_dir(&self.projects).with_context(|| format!("reading {}", self.projects.display()))? {
            let project = project?.path();
            let Some(encoded) = project.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                continue;
            };
            let transcripts = project.join("agent-transcripts");
            let Ok(entries) = fs::read_dir(&transcripts) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(composer_id) = entry.file_name().to_str() {
                    map.insert(
                        composer_id.to_owned(),
                        (
                            encoded.clone(),
                            entry.path().join(format!("{composer_id}.jsonl")),
                        ),
                    );
                }
            }
        }
        Ok(map)
    }
}

fn decode(encoded: &str) -> Option<String> {
    let mut segments = encoded.split('-');
    let first = segments.next().filter(|s| !s.is_empty())?;
    let rest: Vec<&str> = segments.collect();
    let mut path = format!("/{first}");
    let mut visits = 0;
    search(&mut path, &rest, &mut visits)
}

fn search(path: &mut String, segments: &[&str], visits: &mut usize) -> Option<String> {
    if *visits >= 10_000 {
        return None;
    }
    *visits += 1;
    let Some((next, rest)) = segments.split_first() else {
        return Path::new(path.as_str()).is_dir().then(|| path.clone());
    };
    for separator in ['/', '.', '-'] {
        if separator == '/' && !Path::new(path.as_str()).is_dir() {
            continue;
        }
        let len = path.len();
        path.push(separator);
        path.push_str(next);
        if let Some(found) = search(path, rest, visits) {
            return Some(found);
        }
        path.truncate(len);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(path: &Path) -> String {
        path.to_str()
            .unwrap()
            .trim_start_matches('/')
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }

    #[test]
    fn decode_round_trips_dots_and_dashes_against_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("code/github.com/supabit-app/session-picker");
        fs::create_dir_all(&real).unwrap();
        let canonical = fs::canonicalize(&real).unwrap();
        assert_eq!(decode(&encode(&canonical)), Some(canonical.to_str().unwrap().to_owned()));
    }

    #[test]
    fn decode_fails_gracefully_for_non_paths() {
        assert_eq!(decode("empty-window"), None);
        assert_eq!(decode("1783374428344"), None);
        assert_eq!(decode(""), None);
    }
}
