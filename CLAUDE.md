# CLAUDE.md — Shannon build guide

You are picking up Shannon cold. **Read [`ARCHITECTURE.md`](./ARCHITECTURE.md) in
full before writing any code.** This file is the short operational guardrail; the
architecture doc is the source of truth and reasoning.

## Read order

1. `ARCHITECTURE.md` (full design + the reasoning behind each decision)
2. This file (conventions + guardrails)
3. Octarine is consumed as a **published dependency** (git tag in `Cargo.toml`), not
   a local checkout — see `ARCHITECTURE.md` §12. Read its API on GitHub
   (<https://github.com/joshjhall/octarine>) / docs.rs as needed. You do NOT need
   local repo access.

## The do-NOT list (these are settled — do not re-litigate)

- Do NOT do transparent TLS MITM or inject a root CA. Use the `ANTHROPIC_BASE_URL`
  override path.
- Do NOT clobber an existing `ANTHROPIC_BASE_URL` — read it, store it as the
  session upstream, and chain to it (Josh routes through Bifrost).
- Do NOT build compaction detection. The Messages API is stateless; re-inject the
  instruction every request instead.
- Do NOT infer session identity from traffic shape. Identity comes from the
  `/s/<uuid>` URL path set by the launch shim.
- Do NOT make a Claude Code plugin/hook the critical path (it runs too late to set
  the base URL). Shim first; hook only as optional secondary enrichment.
- Do NOT store the API key. Forward `Authorization`/`x-api-key` per request,
  untouched.
- Do NOT log request/response bodies, ever. Apply octarine PII redaction to
  Shannon's own logs.
- Do NOT ship free-text NER PII in MVP. Structured detection only.
- Do NOT oversell "encrypted in memory" against a same-UID local attacker — see the
  threat-model boundary in `ARCHITECTURE.md` §7.

## Build order (follow the validation gates; each is go/no-go)

1. Finish + unit-test octarine `StateStore` in-memory backend (#540) and
   `InstanceCounter` token operator (#543) **standalone**. No proxy yet.
2. Traffic-capture harness: pass-through logging proxy + launch shim. Confirm
   base-URL chaining, capture real SSE fixtures, verify `/s/<uuid>` scheme, find
   client `cache_control` breakpoints.
3. Streaming rehydrate spike: SSE boundary state machine wrapping octarine
   deanonymize. Fuzz chunk splits in `text_delta` and `partial_json`. Gate: zero
   half-token leaks.
4. Token-survival measurement: 20–30 realistic coding tasks with seeded secrets +
   chosen sentinel + injected instruction. Measure verbatim round-trip vs mangle.
5. Cost/cache delta: with/without proxy.

Steps 3 and 4 are make-or-break. Do not over-invest in polish before they pass.

## Tech conventions

- **Rust.** Single static binary. `mlock`/`VirtualLock` for secret memory.
- Shannon is its **own crate**, depending on octarine via git tag — NOT an octarine
  workspace member (octarine forbids `unsafe`; keep proxy concerns separate).
- HTTP: Axum/Tower (octarine `http` feature) inbound; `reqwest` (rustls) upstream.
- Octarine deps: `anonymize` (vault + operators), `crypto/secrets`, `identifiers`,
  `crypto/validation`, `observe`, `http`.
- Async: tokio.
- Errors: prefer octarine's `Problem` type where it fits; otherwise `thiserror`.

## Session model (the core mechanic)

- One always-on daemon, many sessions keyed by `/s/<uuid>` URL path.
- Launch shim (per `claude` invocation): mint UUID → register original upstream →
  export `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>/s/<uuid>` → exec claude → on
  exit `shannon notify-end <uuid>` (zeroize vault). Daemon TTL backstop for crashes.
- Per-session vault persistence is mandatory (deterministic tokenization →
  cache stability + token consistency across the replayed history).

## Instruction injection (settled)

- Inject a **byte-stable `system` block**, identical from request #1, every request.
- Goal: it lands inside the prompt-cache prefix → one-time cost, ~cache-read price
  after. Keep it terse. Draft text is in `ARCHITECTURE.md` §3.3.

## Token sentinel

- Avoid octarine's default `<TYPE_n>` (collides with generics/JSX/XML in code).
- Use a low-collision, hard-to-mangle form, e.g. `⟦SSN·4f2a⟧`. The exact choice is
  an input to the step-4 survival test — pick deliberately, then measure.

## Definition of done for MVP

- Claude Code on macOS + Debian runs transparently through Shannon.
- Structured secrets/PII never reach the upstream in plaintext (verified against
  captured fixtures).
- Tokens round-trip verbatim at an acceptable rate (step 4 gate).
- No half-token leaks in streamed responses (step 3 gate).
- Daemon: loopback-only, path-capability gated, no key storage, no body logs,
  vault mlock'd + zeroized on session end.
