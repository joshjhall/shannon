# Finding: token sentinel format

Status: chosen (issue #9). Final id form pending empirical confirmation (#13).

## Chosen grammar

```text
⟦{TYPE}·{id}⟧
```

| Part    | Value                | Notes                                              |
| ------- | -------------------- | -------------------------------------------------- |
| open    | `⟦` U+27E6 (E2 9F A6) | mathematical white square bracket                  |
| `{TYPE}`| `[A-Z][A-Z0-9_]*`    | entity type, e.g. `SSN`, `API_KEY`, `PRIVATE_KEY`  |
| sep     | `·` U+00B7 (C2 B7)   | middle dot                                         |
| `{id}`  | `[0-9a-f]+`          | stable per-session id (decimal index OR hex hash)  |
| close   | `⟧` U+27E7 (E2 9F A7) | mathematical white square bracket                  |

Example: `⟦API_KEY·0⟧`, `⟦SSN·4f2a⟧`.

Implemented in `src/sentinel/`: `Sentinel::render`, `find_complete` (whole-string
extraction), and `safe_flush_boundary` (the streaming primitive #7 depends on).

## Why not octarine's default `<{entity_type}_{index}>`

`<SSN_0>` is actively dangerous in a **coding** context (ARCHITECTURE.md §4.4):

- `<…>` collides with generics (`Vec<T>`), JSX/TSX, and XML/HTML — the model
  sees the delimiter constantly and may reformat or complete it.
- The trailing `_n` reads like an identifier the model is free to rename, split,
  or renumber.

The guillemet + middle-dot delimiters are balanced, visually atomic, and appear
essentially never in source, so the model is far less likely to split or mangle
them, and the rehydrate scanner has an unambiguous boundary to lock onto.

## The octarine `{index}` vs hash constraint (headline finding)

ARCHITECTURE.md's draft `⟦SSN·4f2a⟧` implies the id is a **content hash**. But
octarine's `InstanceCounterAnonymizer::with_format(fmt)` (octarine #543)
substitutes only `{entity_type}` and `{index}`, where `{index}` is an
**incrementing integer** — it cannot emit a hash. So a token minted directly by
octarine is `⟦SSN·0⟧`, `⟦SSN·1⟧`, …

Rather than fight octarine or post-process every token, `{id}` is defined as
`[0-9a-f]+`, which matches **both** a decimal index and a lowercase-hex hash.
Consequences:

- MVP wires `OCTARINE_FORMAT = "⟦{entity_type}·{index}⟧"` straight into the vault
  (#2) — zero post-processing, deterministic within a session.
- If #13 shows the incrementing index hurts survival (e.g. the model "helpfully"
  renumbers), a hex hash can be swapped in **without changing the matcher** —
  `safe_flush_boundary` / `find_complete` already accept it. That swap would move
  token-minting off octarine's format into a thin Shannon operator.

The id form is therefore an **input to the #13 A/B**, not a settled choice.

## Delimiter alternatives considered

| Candidate         | Rejected because                                             |
| ----------------- | ------------------------------------------------------------ |
| `<TYPE_n>`        | octarine default; collides with generics/JSX/XML (see above) |
| `[[TYPE:id]]`     | `[[` `]]` collide with wiki-links / some templating / TOML   |
| `{{TYPE:id}}`     | collides with Handlebars/Jinja/Go-template mustaches         |
| `§TYPE.id§`       | `§` is single-byte-ish in Latin-1 mindsets; less "atomic"    |
| `⟦TYPE·id⟧` (chosen) | multi-byte, balanced, near-zero source collision          |

A multi-byte delimiter also *helps* the streaming scanner: a split mid-delimiter
(`E2 9F …`) is detectable as a partial sentinel at the byte level, which is
exactly what `safe_flush_boundary` keys on.

## Open follow-ups

- **#13** — A/B the chosen sentinel (and index-vs-hash id) across 20–30 realistic
  tasks; lock the final id form based on measured verbatim round-trip rate.
- **#7** — build the SSE boundary state machine on `safe_flush_boundary`.
- **#2** — pass `OCTARINE_FORMAT` to the vault's `InstanceCounter` operators.
