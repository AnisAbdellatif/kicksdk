//! Chat message text.
//!
//! Kick sends emotes inline, as `[emote:<id>:<name>]`, and leaves drawing
//! them to the client. [`parse`] splits a message into text and emotes.

use serde::Serialize;

/// One piece of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// An emote.
    Emote {
        /// Kick's emote id.
        id: String,
        /// Its name.
        name: String,
        /// How many in a row, with `combine`; 1 otherwise.
        count: u32,
    },
}

/// Splits `content` into text and emotes. With `combine`, a run of the same
/// emote - back to back or with only whitespace between - becomes one part
/// with a count, so `KEKW KEKW KEKW` is one emote, ×3. The same emote means
/// the same id; the name is the first one's.
pub fn parse(content: &str, combine: bool) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let mut rest = content;
    let mut between = String::new();
    while let Some(start) = rest.find("[emote:") {
        let (before, token) = rest.split_at(start);
        let Some((id, name, len)) = emote_token(token) else {
            between.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        };
        between.push_str(before);
        let same_run = combine
            && between.trim().is_empty()
            && matches!(parts.last(), Some(Part::Emote { id: last, .. }) if *last == id);
        if same_run {
            if let Some(Part::Emote { count, .. }) = parts.last_mut() {
                *count += 1;
            }
        } else {
            if !between.is_empty() {
                parts.push(Part::Text { text: std::mem::take(&mut between) });
            }
            parts.push(Part::Emote { id, name, count: 1 });
        }
        between.clear();
        rest = &token[len..];
    }
    between.push_str(rest);
    if !between.is_empty() {
        parts.push(Part::Text { text: between });
    }
    parts
}

/// `[emote:<digits>:<name>]` at the start of `s`: (id, name, its length).
fn emote_token(s: &str) -> Option<(String, String, usize)> {
    let body = s.strip_prefix("[emote:")?;
    let colon = body.find(':')?;
    let id = &body[..colon];
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let after = &body[colon + 1..];
    let close = after.find(']')?;
    let name = &after[..close];
    Some((id.to_owned(), name.to_owned(), "[emote:".len() + colon + 1 + close + 1))
}

/// An emote's image.
pub fn emote_url(id: &str) -> String {
    format!("https://files.kick.com/emotes/{}/fullsize", crate::http::encode(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emote(id: &str, name: &str, count: u32) -> Part {
        Part::Emote { id: id.into(), name: name.into(), count }
    }
    fn text(t: &str) -> Part {
        Part::Text { text: t.into() }
    }

    #[test]
    fn splits_text_and_emotes() {
        assert_eq!(
            parse("hi [emote:37226:KEKW] there", false),
            vec![text("hi "), emote("37226", "KEKW", 1), text(" there")]
        );
        assert_eq!(parse("", false), vec![]);
        assert_eq!(parse("[emote:x:no] [emote:1:", false), vec![text("[emote:x:no] [emote:1:")]);
    }

    #[test]
    fn combines_runs_of_one_emote() {
        assert_eq!(
            parse("[emote:1:A] [emote:1:A][emote:1:B] [emote:2:C] x [emote:2:C]", true),
            vec![emote("1", "A", 3), text(" "), emote("2", "C", 1), text(" x "), emote("2", "C", 1)]
        );
        assert_eq!(parse("[emote:1:A] [emote:1:A]", false).len(), 3);
    }
}
