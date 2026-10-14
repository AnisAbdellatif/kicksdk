//! Chat message text.
//!
//! Kick sends emotes inline, as `[emote:<id>:<name>]`, and leaves drawing
//! them to the client. [`parse`] splits a message into text and emotes.

use serde::Serialize;

/// One piece of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
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
///
/// Linear in the length of `content`, whatever it holds: a name ends at the
/// first `]` and cannot contain `[`, so no stretch of text is scanned twice.
pub fn parse(content: &str, combine: bool) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let mut rest = content;
    let mut between = String::new();
    while let Some(start) = rest.find("[emote:") {
        let (before, token) = rest.split_at(start);
        let (id, name, len) = match emote_token(token) {
            Token::Emote(id, name, len) => (id, name, len),
            Token::Not => {
                between.push_str(&rest[..start + 1]);
                rest = &rest[start + 1..];
                continue;
            }
            // No `]` anywhere further on: nothing after this can be an emote.
            Token::Unclosed => break,
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

enum Token {
    /// (id, name, its length).
    Emote(String, String, usize),
    /// Not an emote; one may start further on.
    Not,
    /// An emote's start, and no `]` anywhere after it.
    Unclosed,
}

/// `[emote:<digits>:<name>]` at the start of `s`. Scans only the id's digits
/// and the name up to the first `[` or `]`, so the work done on a failed
/// token is never repeated for the next one.
fn emote_token(s: &str) -> Token {
    let Some(body) = s.strip_prefix("[emote:") else { return Token::Not };
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || body.as_bytes().get(digits) != Some(&b':') {
        return Token::Not;
    }
    let after = &body[digits + 1..];
    match after.find(['[', ']']) {
        Some(close) if after.as_bytes()[close] == b']' => {
            Token::Emote(body[..digits].to_owned(), after[..close].to_owned(), "[emote:".len() + digits + 1 + close + 1)
        }
        Some(_) => Token::Not,
        None => Token::Unclosed,
    }
}

/// An emote's image. `id` is Kick's emote id (digits); anything else would
/// not name an emote, and `.`, `..` or an empty id would move the URL's
/// path, so it is refused.
pub fn emote_url(id: &str) -> Result<String, crate::Error> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::Error::Invalid(format!("emote id {id:?}: digits only")));
    }
    Ok(format!("https://files.kick.com/emotes/{id}/fullsize"))
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

    #[test]
    fn broken_tokens_stay_text() {
        assert_eq!(parse("[emote:1:a[emote:2:B]", false), vec![text("[emote:1:a"), emote("2", "B", 1)]);
        assert_eq!(parse("[emote::x] [emote:1x:y]", false), vec![text("[emote::x] [emote:1x:y]")]);
        assert_eq!(parse("[emote:1:A] [emote:2:", false), vec![emote("1", "A", 1), text(" [emote:2:")]);
    }

    #[test]
    fn unclosed_tokens_take_linear_time() {
        // Quadratic scanning would take minutes on this; linear is instant.
        let evil = "[emote:1:".repeat(200_000);
        let start = std::time::Instant::now();
        assert_eq!(parse(&evil, true), vec![text(&evil)]);
        let evil = "[emote:1:a".repeat(200_000) + "]";
        let parts = parse(&evil, true);
        assert_eq!(parts.len(), 2);
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn emote_urls_take_digits_only() {
        assert_eq!(emote_url("37226").unwrap(), "https://files.kick.com/emotes/37226/fullsize");
        for bad in ["", ".", "..", "1/2", "a"] {
            assert!(emote_url(bad).is_err(), "{bad:?}");
        }
    }
}
