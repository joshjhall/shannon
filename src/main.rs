//! Shannon — local hardened redaction proxy.
//!
//! Sits between an LLM coding client (Claude Code first) and its upstream API
//! endpoint. On the way up it detects secrets/PII and replaces them with
//! reversible tokens; on the way down it rehydrates them before the client
//! sees the response. See `ARCHITECTURE.md` for the full design.
//!
//! This is a scaffold entry point — the daemon, launch shim, and streaming
//! rehydrate state machine are built per the validation gates in `CLAUDE.md`.

fn main() {
    println!("shannon: scaffold — see ARCHITECTURE.md for the build plan");
}
