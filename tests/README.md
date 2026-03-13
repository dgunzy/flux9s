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
├── snapshots/
│   └── *.snap
└── unit/
    └── mod.rs
```

Most tests are top-level integration-style Rust test binaries, not nested `tests/unit/...` or `tests/integration/...` trees.

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
# Default contributor check
just ci

# Library + unit-style tests
just test

# Selected integration-style tests used in CI
just test-integration

# All test binaries directly
cargo test --lib --tests
```

TUI-specific tests such as `snapshot_tests` and `navigation_tests` are feature-gated in `Cargo.toml` and require the default `tui` feature.

## Snapshots

Snapshot files are stored under `tests/snapshots/` and are validated by `snapshot_tests.rs`. When intentional rendering changes occur, review the updated snapshots carefully before accepting them.
