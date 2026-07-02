//! Shannon — local hardened redaction proxy (library crate).
//!
//! Shannon sits between an LLM coding client (Claude Code first) and its
//! upstream API endpoint. On the way up it detects secrets/PII and replaces
//! them with reversible **sentinel tokens**; on the way down it rehydrates
//! them before the client sees the response. See `ARCHITECTURE.md` for the
//! full design and `CLAUDE.md` for the build order.
//!
//! This library exposes the reusable pieces; the `shannon` binary
//! (`src/main.rs`) is a thin entry point on top of it. Modules are added as
//! the validation gates land — `sentinel` is the first.

pub mod sentinel;
