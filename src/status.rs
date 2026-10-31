//! Program status (OSC 7501): a program tells its terminal what it is doing,
//! such as working, blocked on you, done or failed. Spec:
//! https://mitchellh.com/writing/program-status-osc7501
//!
//! `ESC ] 7501 ; state=blocked:kind=permission:app=terraform:msg=<base64> ST`

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Records kept per terminal. The spec asks for at least 64.
const MAX_RECORDS: usize = 64;
/// Limits from the spec; a report that breaks one is discarded whole.
const MAX_REPORT: usize = 4096;
const MAX_KEY: usize = 16;
const MAX_MSG: usize = 2732;
const MAX_TITLE: usize = 256;
const MAX_NAME: usize = 32;
const MAX_ID: usize = 128;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl Status {
    /// Which record a terminal shows first: the one that needs you most.
    fn urgency(self) -> u8 {
        match self {
            Status::Blocked => 4,
            Status::Error => 3,
            Status::Working => 2,
            Status::Done => 1,
            Status::Idle => 0,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Record {
    pub status: Status,
    /// What a blocked program waits for: permission, question or auth.
    pub kind: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// When it was last reported, to show the latest of equal records and
    /// to evict the stalest.
    seq: u64,
}

#[derive(PartialEq, Debug)]
pub enum Report {
    Set { id: String, record: Record },
    /// Remove the record and the ones beneath it; the root (empty id)
    /// removes every record.
    Clear { id: String },
}

/// Parse the body of an OSC 7501 report, after `7501;`.
pub fn parse(body: &[u8]) -> Option<Report> {
    if body.len() > MAX_REPORT {
        return None;
    }
    let body = std::str::from_utf8(body).ok()?;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if key.is_empty() || key.len() > MAX_KEY || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b)) {
            continue;
        }
        fields.insert(key, value);
    }

    let id = fields.get("id").copied().unwrap_or("");
    if !valid_id(id) {
        return None;
    }
    let status = match *fields.get("state")? {
        "clear" => return Some(Report::Clear { id: id.into() }),
        "idle" => Status::Idle,
        "working" => Status::Working,
        "done" => Status::Done,
        "blocked" => Status::Blocked,
        "error" => Status::Error,
        _ => return None,
    };
    let kind = fields
        .get("kind")
        .filter(|k| status == Status::Blocked && matches!(**k, "permission" | "question" | "auth"))
        .map(|k| k.to_string());
    let app = fields.get("app").filter(|a| valid_name(a)).map(|a| a.to_string());
    let text = |key: &str, max: usize| -> Option<Option<String>> {
        match fields.get(key) {
            None => Some(None),
            Some(v) if v.len() > max => None,
            Some(v) => text(v).map(Some),
        }
    };
    let title = text("title", MAX_TITLE)?;
    let msg = text("msg", MAX_MSG)?;
    Some(Report::Set { id: id.into(), record: Record { status, kind, app, title, msg, seq: 0 } })
}

fn valid_name(segment: &str) -> bool {
    (1..=MAX_NAME).contains(&segment.len()) && segment.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

/// Empty is the root record; otherwise segments joined by `/`.
fn valid_id(id: &str) -> bool {
    id.is_empty() || (id.len() <= MAX_ID && id.split('/').count() <= MAX_DEPTH && id.split('/').all(valid_name))
}

/// Base64 text, refused if it doesn't decode or holds control characters.
fn text(encoded: &str) -> Option<String> {
    let text = String::from_utf8(base64(encoded)?).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// Standard base64; padding is optional.
fn base64(encoded: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let (mut bits, mut count) = (0u32, 0);
    for b in encoded.trim_end_matches('=').bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// What the programs in one terminal reported, one record per id.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Records {
    records: HashMap<String, Record>,
    seq: u64,
}

impl Records {
    pub fn apply(&mut self, report: Report) {
        match report {
            Report::Clear { id } if id.is_empty() => self.records.clear(),
            Report::Clear { id } => {
                let below = format!("{id}/");
                self.records.retain(|key, _| *key != id && !key.starts_with(&below));
            }
            Report::Set { id, mut record } => {
                self.seq += 1;
                record.seq = self.seq;
                if !self.records.contains_key(&id) && self.records.len() >= MAX_RECORDS {
                    let stalest = self.records.iter().min_by_key(|(_, r)| r.seq).map(|(k, _)| k.clone());
                    self.records.remove(&stalest.unwrap_or_default());
                }
                self.records.insert(id, record);
            }
        }
    }

    /// The program exited or the shell shows a new prompt: whatever was
    /// working or waiting is over. Results (done, error) stay to be read.
    pub fn end_runs(&mut self) {
        self.records.retain(|_, r| !matches!(r.status, Status::Working | Status::Blocked));
    }

    /// The record that needs you most (the latest of equals), and a line
    /// that says why.
    pub fn summary(&self) -> Option<(Status, String)> {
        let record = self.records.values().max_by_key(|r| (r.status.urgency(), r.seq))?;
        let detail = record.msg.clone().or_else(|| record.kind.clone()).or_else(|| record.title.clone()).unwrap_or_default();
        Some((record.status, detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(body: &str) -> Record {
        match parse(body.as_bytes()) {
            Some(Report::Set { record, .. }) => record,
            other => panic!("{body}: {other:?}"),
        }
    }

    #[test]
    fn parses_reports() {
        // "Apply 3 to add, 1 to change, 0 to destroy?"
        let r = set("state=blocked:kind=permission:app=terraform:msg=QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/");
        assert_eq!(r.status, Status::Blocked);
        assert_eq!(r.kind.as_deref(), Some("permission"));
        assert_eq!(r.app.as_deref(), Some("terraform"));
        assert_eq!(r.msg.as_deref(), Some("Apply 3 to add, 1 to change, 0 to destroy?"));
        // Padding is optional, whitespace is trimmed, a kind only goes with blocked.
        assert_eq!(set(" state = working : kind=auth : msg=SGk ").msg.as_deref(), Some("Hi"));
        assert_eq!(set("state=working:kind=auth").kind, None);
        // Malformed pairs and unknown keys are skipped.
        assert_eq!(set("state=done:junk:Upper=x:future=1").status, Status::Done);
        assert_eq!(parse(b"state=clear:id=build/test"), Some(Report::Clear { id: "build/test".into() }));
    }

    #[test]
    fn discards_bad_reports_whole() {
        for body in [
            "msg=SGk",                    // no state
            "state=sleeping",             // a state from the future
            "state=done:msg=SGk",         // fine, but:
            "state=done:msg=SGkKdGhlcmU", // "Hi\nthere": a control character
            "state=done:msg=SGk=x",       // not base64
            "state=done:id=a//b",         // empty id segment
        ] {
            assert_eq!(parse(body.as_bytes()).is_some(), body == "state=done:msg=SGk", "{body}");
        }
        let long = format!("state=done:title={}", "A".repeat(MAX_TITLE + 4));
        assert!(parse(long.as_bytes()).is_none());
    }

    #[test]
    fn shows_the_record_that_needs_you_most() {
        let mut records = Records::default();
        assert_eq!(records.summary(), None);
        records.apply(parse(b"state=working:app=deploy:msg=RGVwbG95aW5n").unwrap());
        records.apply(parse(b"state=blocked:kind=permission:id=eu-west").unwrap());
        records.apply(parse(b"state=working:id=us-east").unwrap());
        assert_eq!(records.summary(), Some((Status::Blocked, "permission".into())));
        records.apply(parse(b"state=clear:id=eu-west").unwrap());
        assert_eq!(records.summary().unwrap().0, Status::Working);
        // A new prompt ends runs; a result stays.
        records.apply(parse(b"state=done:id=us-east:msg=SGVhbHRoeQ").unwrap());
        records.end_runs();
        assert_eq!(records.summary(), Some((Status::Done, "Healthy".into())));
        records.apply(parse(b"state=clear").unwrap());
        assert_eq!(records.summary(), None);
    }

    #[test]
    fn evicts_the_stalest_record() {
        let mut records = Records::default();
        for i in 0..=MAX_RECORDS {
            records.apply(parse(format!("state=working:id=job{i}").as_bytes()).unwrap());
        }
        assert_eq!(records.records.len(), MAX_RECORDS);
        assert!(!records.records.contains_key("job0"));
    }
}
