# Shannon — Architecture & Handoff Spec

> Status: pre-implementation design. Author: Josh + Adora (2026-06-27).
> Purpose of this doc: capture every conclusion from the feasibility discussion
> so a fresh Claude Code instance can pick it up in a new repo and start building
> without re-deriving the reasoning.

---

## 1. What Shannon is

A small, local, hardened proxy that sits between an LLM coding client (Claude Code
first; Cursor, Claude GUI, Codex, OpenClaw later) and its upstream API endpoint.

On the way **up** (client → cloud) it detects secrets and PII, replaces them with
reversible redaction tokens, and injects a standing instruction telling the model
how to treat those tokens. On the way **down** (cloud → client) it rehydrates the
tokens back to their real values before the client sees the response.

Net effect: the client behaves exactly as it does today; **the cloud/API never
receives secrets or PII**. The plaintext↔token map lives only in encrypted,
mlock'd memory and is never written to disk.

Naming: binary/daemon = `shannon` (Claude Shannon / information theory; pairs with
"Claude"). Product framing TBD ("Shannon Shield" etc.) — keep the daemon name plain.

---

## 2. Hard requirements

1. **Auto-start with the client.** No manual daemon babysitting.
2. **Cross-platform.** MVP: macOS + Debian. Later: Windows.
3. **Respect existing `ANTHROPIC_BASE_URL` overrides** — must *chain* to them, not
   clobber. (Josh routes Claude through Bifrost; Shannon sits in front of Bifrost.)
4. **Multiple client instances per machine**, served by **one** daemon process
   (no duplicated executable in memory), with strict per-session isolation.

Topology (Josh's actual setup):

```text
Claude Code → shannon (127.0.0.1) → Bifrost → Anthropic
```

Shannon reads the *pre-existing* base URL (Bifrost), stores it as the session's
upstream, then points the client at itself.

---

## 3. Key findings that shape the design (read before building)

### 3.1 Use base-URL override, NOT TLS MITM

Claude Code respects `ANTHROPIC_BASE_URL`. Point the client at a local Shannon
endpoint; Shannon is the TLS terminator for the local hop and holds the real
upstream connection. **No injected root CA, no cert-pinning fight, no EDR alarms.**
Transparent MITM is explicitly rejected.

### 3.2 The Messages API is STATELESS

Claude Code keeps no server-side conversation state. **Every request resends the
full `messages` array + system prompt.** Consequences:

- **Instruction injection is per-request and idempotent**, not once-at-start.
  Shannon re-asserts the redaction instruction on *every* outbound request. The
  client never stores or sees it, so it can't be mangled by the client.
- **Compaction cannot strip the instruction.** A freshly-compacted short array is
  just another request; Shannon injects identically. **Do NOT build compaction
  detection.** It was only needed under a (false) stateful model.
- Do NOT rely on any "compaction fingerprint" for correctness. At most a soft
  heuristic for vault GC, never a dependency.

### 3.3 Minimize token overhead via prompt caching

Because the instruction ships on every call, make it a **byte-stable block in the
`system` array**, identical from request #1. Anthropic prompt caching keys on
content-prefix hashes → pay full input price once (first request), ~cache-read
price (~10%) thereafter. One-time cache miss when Shannon first comes online
(it shifts the client's cache breakpoints); stable after. Keep the text terse.

Draft instruction (tune during validation):
> Tokens of the form `⟦…⟧` are opaque placeholders for redacted values. Reproduce
> them exactly when referring to that value; never split, reformat, encode,
> translate, or invent them.

Prefer `system` over a synthetic user/assistant turn (a fake assistant turn risks
voice contamination and is semantically wrong; `system` is for standing rules and
is easier to keep cache-stable).

### 3.4 Octarine already provides most of the secure core

Octarine ([github.com/joshjhall/octarine](https://github.com/joshjhall/octarine),
Rust, a Presidio-anonymizer port) ships:

- **Reversible pseudonymization vault** — `anonymize/vault/` : `StateStore` /
  `SessionId` / `EntityKey`, `(session, original) → stable token`. Async engine
  path `anonymize_async` / `deanonymize_async` is **proven in tests**
  (`anonymize/engine.rs:~1056` mints `<TYPE_n>` tokens and round-trips).
- **AEAD operators** — ChaCha20-Poly1305 / AES-256-GCM, AAD-bound to entity type,
  optional deterministic mode (`anonymize/operators/{encrypt,decrypt}.rs`).
- **mlock'd secret storage, zeroize-on-drop** — `crypto/secrets/{mlock,secure_var,
  map,buffer}.rs`.
- **Structured detectors / validators** — `identifiers/` (financial, biometric,
  credentials, token detection) + `crypto/validation` (PEM/X.509/SSH keys).
- **HTTP middleware** — Axum/Tower stack behind the `http` feature.
- **Observe/PII redaction** for Shannon's own logs.

`unsafe_code = "forbid"` is workspace-wide in octarine (mlock is wrapped safely).
**Shannon is its own crate/binary depending on octarine**, not a workspace member —
keep proxy/networking concerns out of the security lib.

### 3.5 Octarine gaps Shannon must close

- **Vault backend not shipped** — only trait + test mocks. Production in-memory
  store = octarine issue **#540**; `InstanceCounter` token-minting operator = **#543**.
  Finish these *in octarine, standalone, unit-tested* before wiring Shannon.
- **No free-text analyzer.** Octarine *anonymizes spans it's given*; it does not
  scan prose to *find* PII. MVP detection = **structured only** (secrets, API keys,
  JWTs, private keys, SSNs, cards, emails) via `identifiers/` + `crypto/validation`
  - gitleaks-style regex/entropy. Free-text NER PII (names/addresses in comments)
  is **v2** — it's the accuracy hole (false negatives = silent leak; false
  positives = mangled context) and must not block MVP.
- **Streaming rehydrate** — octarine operates on complete strings. The SSE
  response side is novel Shannon code (see §5). Octarine does not help here.

---

## 4. Component architecture

### 4.1 Daemon + launch shim (CHOSEN), with session-id-in-URL-path

Rejected alternatives and why:

- *Pure traffic inference for session identity* — fragile; HTTP keep-alive /
  connection pooling defeats per-connection identity, no stable per-instance body
  field. Avoid.
- *Claude Code plugin/hook as the primary mechanism* — fails on **ordering**, not
  sandboxing: `ANTHROPIC_BASE_URL` is read at process startup, so an in-process
  hook runs too late to set the endpoint. (Hooks *can* curl localhost; they're
  shell commands at the user's UID. A `SessionStart` hook may serve as *secondary*
  enrichment, never the critical path.)

Chosen design:

- **`shannon` daemon** — always on, one process, serves all instances.
- **Launch shim** — a `claude` wrapper (shell function/alias on mac/Linux;
  `.cmd`/PowerShell on Windows). On each invocation it:
  1. mints a session UUID;
  2. reads the existing `ANTHROPIC_BASE_URL` (e.g. Bifrost) and registers it with
     the daemon as this session's upstream;
  3. exports `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>/s/<uuid>` then exec's real
     claude;
  4. on exit, fires `shannon notify-end <uuid>` so the daemon zeroizes that
     session's vault.

Benefits:

- Session identity arrives **in the URL path** — no traffic inference, no pooling
  fragility, no plugin, no ordering problem.
- The UUID is an **unguessable bearer capability** — other local processes can't
  hijack/read a session even if they find the port.
- Process exit = session end = vault flush (lifecycle solved). TTL backstop in the
  daemon for crashes/skipped teardown. Optional `SO_PEERCRED` PID as secondary
  liveness signal.
- One daemon, N `/s/<uuid>` paths = req #4 satisfied by construction.

### 4.2 Request path (up)

1. Receive request on `/s/<uuid>`; resolve session → upstream + vault.
2. Body is a *complete* JSON blob (not streamed) → run octarine detection +
   `anonymize_async` over the full strings. Easy half.
3. Deterministic tokenization within the session (same secret → same token across
   requests) for cache stability and model coherence.
4. Inject/ensure the cache-stable `system` instruction block (§3.3).
5. Forward `Authorization`/`x-api-key` **untouched**; **never store the key**.
6. Send to the session's registered upstream.

### 4.3 Response path (down) — the hard part

SSE-streamed deltas (`content_block_delta` → `text_delta` / `partial_json`).
Tokens can split across chunks (`⟦SEC` … `RET_7⟧`). Shannon needs a **streaming
state machine** that buffers a trailing window, holds back partial token-prefixes,
and only flushes/rehydrates once a boundary is resolved. Same for tool-call
`partial_json`. This is the #1 technical risk; octarine's deanonymize works on
complete text only and must be wrapped.

### 4.4 Token sentinel format (design decision, not cosmetic)

Octarine's default `<TYPE_n>` is dangerous in a *coding* context: `<...>` collides
with generics/JSX/XML; `_n` invites reformatting. Pick a sentinel the model is
least likely to mangle/split/collide with — e.g. guillemet + short hash:
`⟦SSN·4f2a⟧`. The choice directly drives token-survival rate (§6 step 4); test it,
don't guess.

---

## 5. Session & vault lifecycle

- **Per-session vault persistence is mandatory** within a session (deterministic
  tokenization → cache stability + consistent tokens across the replayed history).
- **Eviction trigger = session end** via shim teardown; **TTL backstop** for
  crashes. Compaction is NOT a lifecycle event (§3.2).
- Long sessions accumulate stale entries (secrets compacted out of the convo but
  still in the vault). Acceptable for MVP; bound with TTL/idle GC. Optional later:
  soft compaction heuristic to prune.

---

## 6. Validation plan (ordered go/no-go gates)

1. **Finish + unit-test octarine vault backend (#540) and token operator (#543)
   standalone.** No proxy. *Gate:* two sessions, same secret → distinct,
   non-cross-rehydratable tokens; map zeroizes on drop.
2. **Traffic-capture harness** — pass-through logging proxy (no modification) +
   the launch shim. Confirm base-URL chaining through Bifrost, capture real SSE
   shapes, verify the `/s/<uuid>` session-path scheme, locate client
   `cache_control` breakpoints. *Gate:* clean session identity + recorded fixtures.
3. **Streaming rehydrate spike** — buffer/boundary state machine wrapping octarine
   deanonymize, fuzzed across arbitrary chunk splits in `text_delta` and
   `partial_json`. *Gate:* zero half-token leaks under fuzzing.
4. **Token-survival measurement** — 20–30 realistic coding tasks with seeded
   secrets (read file → refactor → write back → run command using it), with the
   chosen sentinel + injected instruction. Measure verbatim round-trip vs mangle
   rate. *Gate:* mangle rate low enough to be useful. **Empirical make-or-break;
   octarine can't help here.**
5. **Cost/cache delta** — task suite with/without proxy; measure injected-block
   overhead and any cache busting. *Gate:* overhead bounded and predictable.

Steps 3 and 4 are where the project lives or dies. Everything else is plumbing.

---

## 7. Hardening posture (daemon is a high-value target)

It transiently holds plaintext PII *and* sees the API key.

- **Bind 127.0.0.1 only**, random high port; gate every request on a registered
  `/s/<uuid>`; reject unknown/expired session ids; rate-limit. (UDS would give
  `SO_PEERCRED` but the client dials an `http://` URL, so loopback TCP + path
  capability is the practical channel.)
- **Never store the API key** — forward the incoming `Authorization`/`x-api-key`
  per request, untouched. Daemon stays stateless on credentials.
- **Vault in mlock'd, zeroize-on-drop memory** (octarine), AEAD at rest-in-RAM,
  per-session key zeroized on teardown.
- **Never log bodies.** Structured logs with octarine PII redaction applied to
  Shannon's *own* logs — the leak-stopper must not become a leak.
- Least privilege; drop privileges; no disk persistence of secrets/keys ever.

### Honest threat-model boundary

Shannon defends against **the cloud/network seeing plaintext**, plus swap/core-dump
resistance. It does **NOT** defend against a same-UID local attacker who can
`ptrace`/read the daemon's memory — that attacker already owns the Claude process
and the API key. Frame the product this way; do not oversell "encrypted in memory"
as protection against in-process compromise.

---

## 8. Tech choices

- **Language: Rust.** Single static binary, fast startup (wraps every API call),
  `mlock`/`VirtualLock`, and it's what octarine is. Avoid Node for the core.
- **HTTP: Axum/Tower** (octarine `http` feature) + `reqwest` (rustls) upstream.
- **Octarine modules used** (always-compiled, not Cargo features): `anonymize`
  (vault + operators), `crypto::secrets`, `identifiers`, `crypto::validation`,
  `observe` (own-log redaction). The **Cargo features** these need are just
  `observe`, `security`, `http`, and `crypto-validation` (for the JWT/x509/ssh
  detectors) — `anonymize`/`identifiers`/`crypto::secrets` are on by default and
  take no feature flag.
- Shannon = separate crate/binary depending on octarine via git tag.

---

## 9. Roadmap beyond MVP

- Free-text NER PII detection (the accuracy-budgeted v2).
- Additional clients: Cursor, Claude GUI, Codex (different base-URL/env knobs and
  wire formats — each needs its own adapter + survival testing).
- OpenClaw plugin packaging.
- Windows auto-start (Service / Task Scheduler) once mac/Debian are solid.
- Per-OS auto-start: launchd (macOS), systemd user unit (Debian).

---

## 10. Explicit "do NOT" list

- Do NOT do transparent TLS MITM / inject a root CA.
- Do NOT clobber an existing `ANTHROPIC_BASE_URL` — chain to it.
- Do NOT build compaction detection for instruction survival.
- Do NOT infer session identity from traffic shape.
- Do NOT make a Claude Code plugin/hook the critical path (ordering).
- Do NOT store the API key.
- Do NOT log request/response bodies.
- Do NOT ship free-text NER PII in MVP.
- Do NOT oversell "encrypted in memory" vs a same-UID local attacker.

---

## 11. Open questions to resolve early

- Exact client `cache_control` breakpoint layout — does inserting a `system` block
  cache cleanly? (Resolve in step 2.)
- Best sentinel format for survival (step 4 input).
- Idle TTL value + whether `SO_PEERCRED` liveness is worth the complexity.
- Windows launch-shim shape (PowerShell profile fn vs shim exe).
- How other clients (Cursor/Codex) expose base-URL override + whether their wire
  format diverges from Anthropic Messages.

---

## 12. Reference: octarine

Consumed as a published dependency via its normal release process — Shannon does
NOT need local checkout access to the octarine repo. Pin a git tag in `Cargo.toml`:

```toml
[dependencies]
octarine = { git = "https://github.com/joshjhall/octarine", tag = "v0.3.0-beta.4", default-features = false, features = ["observe", "security", "http", "crypto-validation"] }
```

`v0.3.0-beta.4` is the first release whose tree carries the in-memory
`InMemoryStore` (octarine #540) and the `InstanceCounter{Anonymizer,Deanonymizer}`
operators (octarine #543/#653) Shannon's tokenization path needs. Feature note:
`anonymize`/`identifiers`/`crypto` are always-compiled octarine *modules*, not
Cargo features — the vault + operators need no flag; `crypto-validation` gates the
JWT/x509/ssh-key detectors.

- Repo: <https://github.com/joshjhall/octarine>
- Module map (paths within that repo, for orientation only):
  - Anonymize core: `crates/octarine/src/anonymize/` (`engine.rs`, `vault/`, `operators/`)
  - Secret memory: `crates/octarine/src/crypto/secrets/`
  - Detectors: `crates/octarine/src/identifiers/`, `crates/octarine/src/crypto/validation/`
- Octarine issues to finish first (in octarine, released before Shannon depends on
  them): **#540** (in-memory StateStore), **#543** (InstanceCounter token operator).
