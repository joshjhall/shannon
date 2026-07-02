# Shannon

A small, local, hardened proxy that keeps secrets and PII out of LLM coding-tool
API traffic — transparently.

Shannon sits between an LLM coding client (Claude Code first) and its upstream API.
On the way up it detects secrets/PII, swaps them for reversible redaction tokens,
and tells the model how to handle those tokens. On the way down it rehydrates the
tokens to their real values before the client sees them. The client works exactly
as it does today; **the cloud never receives plaintext secrets or PII**.

The plaintext↔token map lives only in encrypted, `mlock`'d memory and is never
written to disk.

> Name: Claude **Shannon** (information theory) — pairs with "Claude".

## Status

Pre-implementation. Design is captured in [`ARCHITECTURE.md`](./ARCHITECTURE.md);
read it in full before writing code. Build conventions and guardrails are in
[`CLAUDE.md`](./CLAUDE.md).

## Development

Shannon develops inside a devcontainer built from the pinned
[`containers`](https://github.com/joshjhall/containers) submodule (Rust + Node +
dev-tools). Open the repo in VS Code (or Zed) with the Dev Containers extension
and reopen in container; the toolchain, linters, and git hooks are provisioned
automatically.

First-time local setup outside the container:

```sh
git submodule update --init containers   # fetch the pinned build/tooling image
cp .env.example .env                      # non-secret config (1Password refs)
cp .env.secrets.example .env.secrets      # add your OP_SERVICE_ACCOUNT_TOKEN
```

Common tasks (run `just --list` for the full set):

| Command | What it does |
| --- | --- |
| `just check` / `just clippy` | Type-check / lint (deny warnings) |
| `just test` | nextest + doctests |
| `just fmt` | `cargo fmt` (JSON/YAML via `just fmt-data`, TOML via `just fmt-toml`) |
| `just bacon` | Continuous inner-loop check/clippy/test |
| `just preflight` | Full pre-push gate (fmt, clippy, lint, spell, test) |
| `just deps-check` | audit + deny + osv + outdated |

Git hooks (`lefthook install`, run automatically in the container) enforce
formatting, spell-check, secret scanning, and conventional-commit messages on
commit/push. Container-only tools skip gracefully on a bare host — CI is the
safety net.

## How it works (one paragraph)

The client respects `ANTHROPIC_BASE_URL`, so Shannon registers itself as the base
URL and chains to the real upstream (e.g. Bifrost → Anthropic) — **no TLS MITM, no
injected CA**. A thin launch shim mints a per-session UUID, registers the original
upstream with the daemon, and points the client at `http://127.0.0.1:<port>/s/<uuid>`.
One always-on daemon serves all client instances; the URL path is the session
identity and an unguessable capability token. The Messages API is stateless (the
client resends full history every call), so Shannon re-asserts a cache-stable
redaction instruction on every request and tokenizes deterministically per session.

## Architecture at a glance

```text
Claude Code → shannon (127.0.0.1/s/<uuid>) → Bifrost → Anthropic
                  │
                  ├─ up:   detect + tokenize (octarine), inject system instruction
                  └─ down: stream-aware rehydrate (SSE boundary state machine)
```

## MVP scope

- **Clients:** Claude Code only.
- **Platforms:** macOS + Debian.
- **Detection:** structured secrets/PII only (API keys, JWTs, private keys, SSNs,
  cards, emails) via octarine. Free-text NER PII is explicitly **out** (v2).

## Built on octarine

[`octarine`](https://github.com/joshjhall/octarine) (Rust security/observability
lib) provides the reversible token vault, AEAD operators, `mlock`'d secret memory,
structured detectors, and HTTP middleware. Shannon depends on it as a published
crate (git tag, via octarine's normal release process — no local checkout needed).
Two octarine pieces must ship first: in-memory `StateStore` (#540) and the
`InstanceCounter` token operator (#543).

## What's hard (and why this might not ship)

1. **SSE streaming rehydrate** — redaction tokens can split across stream chunks;
   needs a boundary-aware buffer. Octarine doesn't help here.
2. **Token survival** — tokenization only works if the model passes tokens through
   verbatim. Must be measured empirically, not assumed.

See the validation gates in `ARCHITECTURE.md` §6.

## Roadmap

Cursor / Claude GUI / Codex adapters · OpenClaw plugin · Windows · free-text NER PII.

## License

TBD (octarine is MIT OR Apache-2.0).
