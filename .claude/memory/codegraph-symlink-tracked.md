---
name: codegraph-symlink-tracked
description: Why .codegraph is committed as a symlink but its index content is not
metadata:
  node_type: memory
  type: project
  originSessionId: 3ea6e0b9-7b7b-4355-9b88-7a146f928a85
---

`.codegraph` is committed to the repo **as a symlink** (git mode 120000 →
`/cache/codegraph`). Only the symlink is versioned; the index it points to
(`codegraph.db`, several MB) is NOT — it lives in a Docker named volume
(`shannon-codegraph`) and is generated per-container by the containers
submodule's first-startup hook (`/etc/container/first-startup/45-codegraph-index.sh`).

**Why:** everyone's `.codegraph` resolves identically, but each dev builds their
own local index cache. The cache survives container rebuilds (named volume) and
never lands in git — so no stale multi-MB DB, no diff churn, no shared-index
drift.

**How to apply:** keep `.gitignore` as `.codegraph/` (trailing slash — matches a
real index *dir* built on a bare host, but NOT the tracked symlink). Do NOT add a
bare `.codegraph` ignore line; that would untrack the symlink. On a fresh clone
where `/cache/codegraph` doesn't exist yet, the symlink dangles until the
first-startup hook creates the volume dir and indexes.
