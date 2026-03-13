# Show available recipes
default:
    @just --list

# CI targets (in order they run in CI)

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Run clippy linter (with CI flags)
clippy:
    cargo clippy --lib --tests -- -D warnings -A clippy::too_many_arguments -A clippy::items-after-test-module -A clippy::type-complexity -A clippy::should-implement-trait -A renamed_and_removed_lints -A clippy::collapsible-if -A clippy::len-zero -A clippy::assertions-on-constants -A dead-code

# Run library and unit tests
test:
    cargo test --lib --tests

# Run integration tests
test-integration:
    cargo test --test crd_compatibility --test resource_registry --test model_compatibility --test field_extraction --test trace_tests --test graph_tests

# Run cargo-audit to check for CVEs (ignores unmaintained warnings)
audit:
    cargo audit --ignore RUSTSEC-2024-0436

# Run all CI checks in order
ci: fmt clippy audit test test-integration

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
