//! Token sentinel format — the surface form of a redaction token and the
//! matcher the rest of Shannon detects it with.
//!
//! # Grammar
//!
//! A sentinel is `⟦{TYPE}·{id}⟧`, e.g. `⟦API_KEY·0⟧` or `⟦SSN·4f2a⟧`:
//!
//! - open `⟦` (U+27E6), close `⟧` (U+27E7), separator `·` (U+00B7) — balanced,
//!   multi-byte delimiters with near-zero collision against ASCII source code.
//!   Deliberately **not** octarine's default `<TYPE_n>`, whose `<…>` collides
//!   with generics/JSX/XML and whose `_n` invites reformatting
//!   (see `ARCHITECTURE.md` §4.4).
//! - `{TYPE}` = `[A-Z][A-Z0-9_]*` — the entity type (`SSN`, `API_KEY`, …).
//! - `{id}` = `[0-9a-f]+` — a stable per-session identifier. Deliberately
//!   permissive: it matches both octarine's decimal `{index}` (the only thing
//!   `InstanceCounter` can mint — octarine #543) and a hex content hash, so the
//!   index-vs-hash choice can be settled empirically in the survival test (#13)
//!   without touching this matcher. See `docs/findings/sentinel.md`.
//!
//! # Streaming
//!
//! The response arrives as arbitrarily-chunked SSE bytes, so a sentinel can be
//! split across chunks. [`safe_flush_boundary`] is the primitive the streaming
//! rehydrate machine (#7) builds on: it reports how much of a byte buffer is
//! provably free of any *partial* sentinel and can be released, holding back the
//! rest until more bytes arrive.

#[cfg(test)]
mod tests;

/// Opening delimiter, `⟦` (U+27E6).
pub const OPEN: char = '⟦';
/// Closing delimiter, `⟧` (U+27E7).
pub const CLOSE: char = '⟧';
/// Type/id separator, `·` (U+00B7).
pub const SEP: char = '·';

/// Format string handed to octarine's `InstanceCounter` operators (#2).
///
/// The vault mints tokens in this grammar: octarine substitutes `{entity_type}`
/// and `{index}`, and the `{index}` lands in this grammar's `{id}` field.
pub const OCTARINE_FORMAT: &str = "⟦{entity_type}·{index}⟧";

// Delimiter byte sequences, resolved at compile time. The scanner works at the
// byte level so a delimiter that is split mid-UTF-8 at a buffer tail is still
// detectable as a partial sentinel.
const OPEN_BYTES: &[u8] = "⟦".as_bytes(); // E2 9F A6
const CLOSE_BYTES: &[u8] = "⟧".as_bytes(); // E2 9F A7
const SEP_BYTES: &[u8] = "·".as_bytes(); // C2 B7

/// A parsed sentinel token: its entity type and identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentinel {
    /// Entity type, e.g. `SSN` or `API_KEY` (`[A-Z][A-Z0-9_]*`).
    pub entity_type: String,
    /// Stable identifier, e.g. `0` or `4f2a` (`[0-9a-f]+`).
    pub id: String,
}

impl Sentinel {
    /// Construct a sentinel from its parts.
    #[must_use]
    pub fn new(entity_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            entity_type: entity_type.into(),
            id: id.into(),
        }
    }

    /// Render the surface form, e.g. `⟦API_KEY·0⟧`.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{OPEN}{}{SEP}{}{CLOSE}", self.entity_type, self.id)
    }
}

/// A complete sentinel located in a string, with its byte span `start..end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Byte offset of the opening delimiter.
    pub start: usize,
    /// Byte offset just past the closing delimiter.
    pub end: usize,
    /// The parsed token.
    pub sentinel: Sentinel,
}

/// Classification of the byte slice starting at a candidate position.
enum Prefix {
    /// A full sentinel occupies `buf[..len]`.
    Complete { len: usize, sentinel: Sentinel },
    /// A proper prefix of a valid sentinel — could still complete with more
    /// bytes. The whole slice is "live" and must be held.
    Incomplete,
    /// Cannot be the prefix of any valid sentinel.
    Invalid,
}

const fn is_type_start(b: u8) -> bool {
    b.is_ascii_uppercase()
}

const fn is_type_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
}

const fn is_id_char(b: u8) -> bool {
    b.is_ascii_digit() || matches!(b, b'a'..=b'f')
}

/// Match the fixed delimiter `delim` at `buf[pos..]`.
///
/// Returns `Ok(new_pos)` on a full match, `Err(true)` when `buf` ends partway
/// through the delimiter (incomplete — still possible), and `Err(false)` on a
/// definite mismatch (invalid).
fn match_delim(buf: &[u8], mut pos: usize, delim: &[u8]) -> Result<usize, bool> {
    for &d in delim {
        match buf.get(pos) {
            None => return Err(true), // ran out mid-delimiter → incomplete
            Some(&b) if b == d => pos += 1,
            Some(_) => return Err(false), // mismatch → invalid
        }
    }
    Ok(pos)
}

/// Classify the slice starting at `buf[0]` against the sentinel grammar.
fn classify(buf: &[u8]) -> Prefix {
    // Fast reject: a sentinel must open with `⟦`, whose first byte is 0xE2.
    match buf.first() {
        Some(&b) if b == OPEN_BYTES[0] => {}
        _ => return Prefix::Invalid,
    }

    // OPEN delimiter.
    let mut pos = match match_delim(buf, 0, OPEN_BYTES) {
        Ok(p) => p,
        Err(true) => return Prefix::Incomplete,
        Err(false) => return Prefix::Invalid,
    };

    // TYPE = [A-Z][A-Z0-9_]* — first char mandatory.
    let type_start = pos;
    match buf.get(pos) {
        None => return Prefix::Incomplete,
        Some(&b) if is_type_start(b) => pos += 1,
        Some(_) => return Prefix::Invalid,
    }
    loop {
        match buf.get(pos) {
            None => return Prefix::Incomplete,
            Some(&b) if is_type_char(b) => pos += 1,
            Some(_) => break, // must be the SEP now
        }
    }
    let type_end = pos;

    // SEP delimiter.
    pos = match match_delim(buf, pos, SEP_BYTES) {
        Ok(p) => p,
        Err(true) => return Prefix::Incomplete,
        Err(false) => return Prefix::Invalid,
    };

    // id = [0-9a-f]+ — at least one char.
    let id_start = pos;
    match buf.get(pos) {
        None => return Prefix::Incomplete,
        Some(&b) if is_id_char(b) => pos += 1,
        Some(_) => return Prefix::Invalid,
    }
    loop {
        match buf.get(pos) {
            None => return Prefix::Incomplete,
            Some(&b) if is_id_char(b) => pos += 1,
            Some(_) => break, // must be the CLOSE now
        }
    }
    let id_end = pos;

    // CLOSE delimiter.
    pos = match match_delim(buf, pos, CLOSE_BYTES) {
        Ok(p) => p,
        Err(true) => return Prefix::Incomplete,
        Err(false) => return Prefix::Invalid,
    };

    // TYPE and id are ASCII by construction, so this never fails.
    let sentinel = Sentinel::new(
        String::from_utf8_lossy(&buf[type_start..type_end]),
        String::from_utf8_lossy(&buf[id_start..id_end]),
    );
    Prefix::Complete { len: pos, sentinel }
}

/// Largest `n` such that `buf[..n]` is valid UTF-8 (i.e. does not end partway
/// through a multi-byte codepoint).
const fn last_utf8_boundary(buf: &[u8]) -> usize {
    match std::str::from_utf8(buf) {
        Ok(_) => buf.len(),
        Err(e) => e.valid_up_to(),
    }
}

/// Find every complete sentinel in `text`, left to right, non-overlapping.
#[must_use]
pub fn find_complete(text: &str) -> Vec<Match> {
    let buf = text.as_bytes();
    let mut matches = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if let Prefix::Complete { len, sentinel } = classify(&buf[i..]) {
            matches.push(Match {
                start: i,
                end: i + len,
                sentinel,
            });
            i += len;
        } else {
            i += 1;
        }
    }
    matches
}

/// Byte offset up to which `buf` can be safely released: `buf[..n]` provably
/// contains no partial sentinel and no split UTF-8 codepoint, while `buf[n..]`
/// must be held until more bytes arrive.
///
/// This is the contract the streaming rehydrate machine (#7) relies on. It is
/// total and single-pass — it never panics and never splits a UTF-8 scalar.
///
/// When `buf` contains no in-progress sentinel and ends on a codepoint
/// boundary, the result is `buf.len()` (release everything).
#[must_use]
pub fn safe_flush_boundary(buf: &[u8]) -> usize {
    let mut i = 0;
    let mut hold = buf.len();
    while i < buf.len() {
        match classify(&buf[i..]) {
            // A finished sentinel is safe; skip past it.
            Prefix::Complete { len, .. } => i += len,
            // A live prefix runs to the end of the buffer — hold from here.
            Prefix::Incomplete => {
                hold = i;
                break;
            }
            // A false start (`⟦` that can't complete) is ordinary text.
            Prefix::Invalid => i += 1,
        }
    }
    // Never release a partial trailing codepoint either.
    hold.min(last_utf8_boundary(buf))
}
