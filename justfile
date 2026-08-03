# Show available recipes
default:
    @just --list

# CI targets
#
# This file is the single source of truth for what "CI" means: the GitHub
# Actions workflow installs `just` and runs `just ci-check`, so a check can
# never exist in one place and not the other. Add a check to `verify` below and
# both local and CI runs pick it up.

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Lint levels live in Cargo.toml's [lints] tables, not on this command line, so
# CI, this recipe, and rust-analyzer agree. --all-targets covers the binary,
# which is not part of the library target and used to be skipped entirely.

# Run clippy over every target, warnings denied
clippy:
    cargo clippy --all-targets -- -D warnings

# `--tests` covers the lib and bin unit tests plus every integration binary, so
# nothing is enumerated (an explicit list silently omits new test files). `--doc`
# is separate because no target selector includes doctests — the headless
# examples in lib.rs and ClusterSession are compiled nowhere else.

# Run all tests, including doctests
test:
    cargo test --tests
    cargo test --doc

# Proves the headless library mode advertised in lib.rs still builds. The bin
# target is skipped automatically via its required-features = ["tui"].

# Check the library builds without the tui feature
check-headless:
    cargo check --no-default-features --all-targets

# Just the integration binaries — a subset of `just test`, for a faster loop.
test-integration:
    cargo test --test '*'

# Build the clusters first: ./scripts/dev-clusters.sh ci

# Run live-cluster regression tests against the dev kind clusters
test-live:
    cargo test --test live_tests -- --ignored --test-threads=1

# No --ignore list: the previous RUSTSEC-2024-0436 / RUSTSEC-2026-0002 entries
# were stale (paste is no longer in the tree, lru is now 0.18.0) and made this
# recipe disagree with the bare `cargo audit` CI used to run.

# Scan dependencies for CVEs
audit:
    cargo audit

# Both `ci` and `ci-check` delegate here, so a check cannot be added to one and
# forgotten in the other.

# Every verification step, in CI order
verify: clippy check-headless audit test

# Format in place, then run every check
ci: fmt verify

# What GitHub Actions runs: `ci`, but unformatted code fails instead of being rewritten
ci-check: fmt-check verify

# Build targets

# Build the project (debug)
build:
    cargo build

# Build the project (release)
build-release:
    cargo build --release

# Install local dependencies for the Hugo docs site
docs-deps:
    cd docs && npm ci && hugo mod get

# Build the Hugo docs site
docs-build:
    cd docs && hugo --minify

# Serve the Hugo docs site locally
docs-serve:
    cd docs && hugo server

# Check the project (without building)
check:
    cargo check

# Development helpers

# Format code
fmt:
    cargo fmt

# Flux model generation

# Fetch CRDs and generate models (full update)
update-flux:
    ./scripts/update-flux.sh

# Download Flux CRDs from GitHub releases
fetch-crds:
    ./scripts/fetch-crds.sh

# Generate Rust models from CRDs using kopium
generate-models:
    ./scripts/generate-models.sh
