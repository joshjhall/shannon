//! Sentinel matcher tests.
//!
//! The load-bearing property is the streaming one: for *any* split of a byte
//! stream, [`safe_flush_boundary`] must never release a partial sentinel, and
//! the released + held bytes must reassemble to the original. The
//! split-at-every-byte tests exercise that exhaustively for representative
//! inputs; #8 later fuzzes it across randomized inputs.

use super::{Sentinel, find_complete, safe_flush_boundary};

#[test]
fn render_round_trips_through_find() {
    let s = Sentinel::new("API_KEY", "0");
    assert_eq!(s.render(), "⟦API_KEY·0⟧");
    let found = find_complete(&s.render());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].sentinel, s);
}

#[test]
fn render_supports_hex_id() {
    assert_eq!(Sentinel::new("SSN", "4f2a").render(), "⟦SSN·4f2a⟧");
}

#[test]
fn finds_multiple_and_adjacent_sentinels() {
    let text = "before ⟦SSN·0⟧ mid ⟦EMAIL·1⟧⟦CARD·a⟧ after";
    let found = find_complete(text);
    let got: Vec<_> = found.iter().map(|m| m.sentinel.clone()).collect();
    assert_eq!(
        got,
        vec![
            Sentinel::new("SSN", "0"),
            Sentinel::new("EMAIL", "1"),
            Sentinel::new("CARD", "a"),
        ]
    );
    // Spans point at the real bytes.
    for m in &found {
        assert_eq!(&text[m.start..m.end], m.sentinel.render());
    }
}

#[test]
fn does_not_match_code_or_prose_lookalikes() {
    // Generics, indexing, JSON braces, and a stray middle-dot in prose must not
    // register as sentinels — this is the whole reason for the delimiter choice.
    for text in [
        "let v: Vec<T> = vec![0]; let m = {x};",
        "fn foo<A, B>(a: A) -> B {}",
        "the ratio a·b is fine in prose",
        "<SSN_0> is octarine's default, not ours",
        "⟦lowercase·0⟧ has a bad TYPE", // type must start [A-Z]
        "⟦SSN0⟧ missing separator",     // no SEP
        "⟦SSN·⟧ empty id",              // id needs ≥1 char
        "⟦SSN·XYZ⟧ non-hex id",         // id is [0-9a-f]
        "⟦SSN·0 unclosed",              // never closes
    ] {
        assert!(
            find_complete(text).is_empty(),
            "unexpected match in: {text:?}"
        );
    }
}

/// For every byte split of `s`, feeding the two halves through the flush
/// boundary must (a) never emit a partial sentinel and (b) losslessly
/// reassemble. Simulates the two-chunk SSE case at every boundary.
fn assert_split_safe(s: &str) {
    let bytes = s.as_bytes();
    for split in 0..=bytes.len() {
        let (head, tail) = bytes.split_at(split);

        // First chunk: release the safe prefix, carry the rest forward.
        let n = safe_flush_boundary(head);
        assert!(n <= head.len());
        let released_1 = &head[..n];
        // Anything released must be valid UTF-8 (no split codepoint).
        assert!(
            std::str::from_utf8(released_1).is_ok(),
            "split codepoint at {split}"
        );
        // The released prefix must contain no partial sentinel: re-scanning it
        // must not itself hold anything back.
        assert_eq!(
            safe_flush_boundary(released_1),
            released_1.len(),
            "released prefix still holds a partial at split {split}"
        );

        // Second chunk = carried bytes + tail. At stream end everything flushes.
        let mut rest = head[n..].to_vec();
        rest.extend_from_slice(tail);
        let m = safe_flush_boundary(&rest);
        // Reassembly is lossless regardless of where the final boundary lands.
        let mut reassembled = released_1.to_vec();
        reassembled.extend_from_slice(&rest);
        assert_eq!(reassembled, bytes, "lossy reassembly at split {split}");
        assert!(m <= rest.len());
    }
}

#[test]
fn split_at_every_byte_never_leaks_partial() {
    assert_split_safe("prefix ⟦API_KEY·0⟧ suffix");
    assert_split_safe("⟦SSN·4f2a⟧");
    assert_split_safe("a⟦EMAIL·1⟧b⟦CARD·2⟧c");
    assert_split_safe("no sentinels here, just text · and <T> and [0]");
    assert_split_safe("trailing open ⟦SSN·0"); // unterminated on purpose
}

#[test]
fn holds_back_partial_open_delimiter() {
    // Buffer ends on the first two bytes of `⟦` (E2 9F ..). Nothing after the
    // plain text may be released.
    let mut buf = b"text ".to_vec();
    buf.extend_from_slice(&"⟦".as_bytes()[..2]);
    let n = safe_flush_boundary(&buf);
    assert_eq!(n, 5, "should hold from the partial delimiter");
    assert_eq!(&buf[..n], b"text ");
}

#[test]
fn holds_back_partial_body_and_separator() {
    // A fully-open sentinel that hasn't closed yet is entirely held.
    for partial in [
        "text ⟦API",
        "text ⟦API_KEY",
        "text ⟦API_KEY·",
        "text ⟦API_KEY·4f",
    ] {
        let buf = partial.as_bytes();
        let n = safe_flush_boundary(buf);
        assert_eq!(&buf[..n], b"text ", "held wrong amount for {partial:?}");
    }
}

#[test]
fn holds_back_partial_separator_bytes() {
    // Buffer ends partway through the multi-byte `·` separator (C2 ..).
    let mut buf = b"x ".to_vec();
    buf.extend_from_slice("⟦SSN".as_bytes());
    buf.extend_from_slice(&"·".as_bytes()[..1]);
    let n = safe_flush_boundary(&buf);
    assert_eq!(&buf[..n], b"x ");
}

#[test]
fn flushes_everything_when_no_partial() {
    let buf = "done ⟦SSN·0⟧ done".as_bytes();
    assert_eq!(safe_flush_boundary(buf), buf.len());
    let plain = b"no sentinels at all";
    assert_eq!(safe_flush_boundary(plain), plain.len());
}

#[test]
fn false_open_is_not_held() {
    // `⟦` followed by something that can't be a sentinel is ordinary text and
    // must not pin the buffer.
    let buf = "before ⟦123 not a type⟧ after".as_bytes();
    assert_eq!(safe_flush_boundary(buf), buf.len());
}
