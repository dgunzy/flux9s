# Testing Guide

This directory contains the repository's test binaries and snapshot artifacts.

## Current Layout

```text
tests/
├── crd_compatibility.rs
├── favorites_tests.rs
├── field_extraction.rs
├── graph_tests.rs
├── model_compatibility.rs
├── navigation_tests.rs
├── reconciliation_history_tests.rs
├── resource_registry.rs
├── snapshot_tests.rs
├── trace_tests.rs
└── snapshots/
    └── *.snap
```

Test binaries here are top-level integration-style tests, not nested `tests/unit/...` or
`tests/integration/...` trees. Unit tests live inline in the module they test, in a
`#[cfg(test)] mod tests` block — the standard Rust convention, and the only way to reach
private items.

## What Each Test Covers

- `crd_compatibility.rs`: status extraction and CRD compatibility expectations
- `model_compatibility.rs`: generated model deserialization compatibility
- `resource_registry.rs`: watcher/resource registry completeness
- `field_extraction.rs`: per-resource field extraction behavior
- `trace_tests.rs`: trace chain discovery
- `graph_tests.rs`: graph building and graph support rules
- `reconciliation_history_tests.rs`: `status.history` extraction behavior
- `favorites_tests.rs`: favorites persistence behavior
- `navigation_tests.rs`: view transitions and keyboard navigation
- `snapshot_tests.rs`: rendered TUI snapshots

## Running Tests

Common commands from the repository root:

```bash
# Default contributor check — formats, then runs everything CI runs
just ci

# Every test: lib + bin unit tests, all integration binaries, and doctests
just test

# Only the integration binaries (a subset of `just test`), for a faster loop
just test-integration

# All test binaries directly
cargo test --tests
```

CI runs `just ci-check`, which is `just ci` with `fmt-check` in place of `fmt`, so
the checks cannot drift apart. Add new checks to the `verify` recipe in the
justfile rather than to `.github/workflows/ci.yml`.

TUI-specific tests such as `snapshot_tests` and `navigation_tests` are feature-gated in `Cargo.toml` and require the default `tui` feature.

## Snapshots

Snapshot files are stored under `tests/snapshots/` and are validated by `snapshot_tests.rs`. When intentional rendering changes occur, review the updated snapshots carefully before accepting them.
