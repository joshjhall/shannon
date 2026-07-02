# Shannon development commands
# Run `just --list` to see all available recipes.

set dotenv-load := false

# Default: run check, clippy, and tests
default: check clippy test

# ─── Build & Check ───────────────────────────────────────────────────────────

# Type-check with all features
check:
    cargo check --all-features

# Build
build:
    cargo build --all-features

# Generate rustdoc with warnings denied (matches CI doc job)
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# ─── Lint ────────────────────────────────────────────────────────────────────

# Run clippy with all targets and features, deny warnings
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run cargo fmt (check only)
fmt-check:
    cargo fmt --all -- --check

# Run cargo fmt (apply fixes)
fmt:
    cargo fmt --all

# Check YAML/JSON formatting via dprint (no fixes)
fmt-data-check:
    dprint check

# Format YAML/JSON via dprint
fmt-data:
    dprint fmt

# Check TOML formatting via taplo (no fixes; config: .taplo.toml)
fmt-toml-check:
    taplo format --check

# Format TOML via taplo (writes; config: .taplo.toml)
fmt-toml:
    taplo format

# Lint TOML schema correctness via taplo (config: .taplo.toml)
lint-toml:
    taplo lint

# ─── Spell check ─────────────────────────────────────────────────────────────

# Spell-check the repo with typos (read-only; non-zero exit on findings)
spell:
    typos

# Apply typos fixes (prompts before writing changes)
spell-fix:
    typos --write-changes

# ─── Test ────────────────────────────────────────────────────────────────────

# Run all tests + doctests (all features). Tests run under cargo-nextest;
# doctests run via `cargo test --doc` because nextest cannot drive doctests.
test: test-nextest test-docs

# Run nextest-managed tests (everything except doctests)
test-nextest:
    cargo nextest run --all-features --build-jobs 4

# Run doctests via cargo (nextest cannot run doctests)
test-docs:
    cargo test --all-features --doc -j4

# Run tests with output visible
test-verbose:
    cargo nextest run --all-features --build-jobs 4 --no-capture

# Run a specific test by name pattern
test-filter PATTERN:
    cargo nextest run --all-features --build-jobs 4 -E 'test(/{{PATTERN}}/)'

# ─── Coverage ────────────────────────────────────────────────────────────────

# Generate LCOV coverage report (target/coverage/lcov.info)
coverage:
    mkdir -p target/coverage
    cargo llvm-cov --all-features -j4 --lcov --output-path target/coverage/lcov.info

# Generate an HTML coverage report (target/llvm-cov/html/index.html)
coverage-html:
    cargo llvm-cov --all-features -j4 --html
    @echo ""
    @echo "Open: target/llvm-cov/html/index.html"

# Print a short coverage summary to the console
coverage-summary:
    cargo llvm-cov --all-features -j4 --summary-only

# ─── Inner-loop ──────────────────────────────────────────────────────────────

# Launch bacon for continuous background checks (config: bacon.toml). Default
# job is `check`; press `c` for clippy, `t` for test, `d` for rustdoc.
bacon *ARGS:
    bacon {{ARGS}}

# ─── Shell / Docker / Markdown / Workflows ───────────────────────────────────

# Lint shell scripts with shellcheck (no-op when none present; submodule excluded)
shellcheck:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(git ls-files | /usr/bin/grep -E '\.sh$' | /usr/bin/grep -v '^containers/' || true)
    if [ -z "$files" ]; then
        echo "shellcheck: no shell scripts to lint"
    else
        echo "$files" | xargs shellcheck
    fi

# Format shell scripts with shfmt (no-op when none present; submodule excluded)
fmt-sh:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(git ls-files | /usr/bin/grep -E '\.sh$' | /usr/bin/grep -v '^containers/' || true)
    if [ -z "$files" ]; then
        echo "shfmt: no shell scripts to format"
    else
        echo "$files" | xargs shfmt -w -i 2 -ci -bn
    fi

# Check shell script formatting with shfmt (no-op when none present)
fmt-sh-check:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(git ls-files | /usr/bin/grep -E '\.sh$' | /usr/bin/grep -v '^containers/' || true)
    if [ -z "$files" ]; then
        echo "shfmt: no shell scripts to check"
    else
        echo "$files" | xargs shfmt -d -i 2 -ci -bn
    fi

# Lint Dockerfiles with hadolint (no-op when none present; submodule excluded)
lint-docker:
    #!/usr/bin/env bash
    set -euo pipefail
    files=$(git ls-files | /usr/bin/grep -E '(^|/)Dockerfile([.-].+)?$' | /usr/bin/grep -v '^containers/' || true)
    if [ -z "$files" ]; then
        echo "hadolint: no Dockerfiles to lint"
    else
        echo "$files" | xargs hadolint
    fi

# Lint Markdown with rumdl (config: .rumdl.toml; submodule excluded there)
lint-md:
    rumdl check .

# Format Markdown with rumdl (writes; review diff before committing)
fmt-md:
    rumdl fmt .

# Lint GitHub Actions workflows with actionlint (embedded shellcheck at warning severity)
lint-workflows:
    actionlint -shellcheck "shellcheck --severity=warning"

# Lint a commit message against the conform policy (default: HEAD's message)
commit-lint FILE='.git/COMMIT_EDITMSG':
    conform enforce --commit-msg-file {{FILE}}

# Lint every commit on this branch against origin/main
commit-lint-branch:
    conform enforce --base-branch origin/main

# ─── Pre-flight (run before push / PR) ──────────────────────────────────────

# Full pre-push validation
preflight: fmt-check fmt-data-check fmt-toml-check fmt-sh-check clippy shellcheck lint-docker lint-md lint-workflows lint-toml lint-deps spell test

# ─── Dependencies ────────────────────────────────────────────────────────────

# Update Cargo.lock to latest semver-compatible versions
deps-update:
    cargo update
    @echo "Run 'just deps-audit' to check for remaining vulnerabilities"

# Show outdated dependencies
deps-outdated:
    cargo outdated --depth 1

# Check for known security vulnerabilities
deps-audit:
    cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2023-0089

# Run cargo-deny checks (advisories, licenses, sources)
deps-deny:
    cargo deny check

# Check the lockfile against OSV.dev (RustSec + GHSA + cross-ecosystem)
deps-osv:
    osv-scanner scan source --lockfile=Cargo.lock

# Full dependency health check: audit + deny + osv + outdated
deps-check: deps-audit deps-deny deps-osv deps-outdated

# Detect unused dependencies. Exit 0 = clean; exit 1 = unused deps found.
lint-deps:
    cargo machete

# ─── Utilities ───────────────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# Run all pre-commit hooks on every file (via lefthook)
pre-commit:
    lefthook run pre-commit --all-files

# Install lefthook git hooks
install-hooks:
    lefthook install
