.PHONY: help update-flux fetch-crds generate-models clean-models build check test ci fmt fmt-check clippy audit

help: ## Show this help message
	@echo "Flux TUI - Build and Maintenance Commands"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-20s %s\n", $$1, $$2}'

# CI targets (in order they run in CI)
fmt-check: ## Check formatting without modifying files
	cargo fmt -- --check

clippy: ## Run clippy linter (with CI flags)
	cargo clippy --lib --tests -- -D warnings -A clippy::too_many_arguments -A clippy::items-after-test-module -A clippy::type-complexity -A clippy::should-implement-trait -A renamed_and_removed_lints -A clippy::collapsible-if -A clippy::len-zero -A clippy::assertions-on-constants -A dead-code

test: ## Run library and unit tests
	cargo test --lib --tests

test-integration: ## Run integration tests
	cargo test --test crd_compatibility --test resource_registry --test model_compatibility --test field_extraction --test trace_tests

audit: ## Run cargo-audit to check for CVEs (ignores unmaintained warnings)
	cargo audit --ignore RUSTSEC-2024-0436

ci: fmt clippy audit test test-integration ## Run all CI checks in order

# Build targets
build: ## Build the project (debug)
	cargo build

build-release: ## Build the project (release)
	cargo build --release

check: ## Check the project (without building)
	cargo check

# Development helpers
fmt: ## Format code
	cargo fmt

# Flux model generation
update-flux: ## Fetch CRDs and generate models (full update)
	@./scripts/update-flux.sh

fetch-crds: ## Download Flux CRDs from GitHub releases
	@./scripts/fetch-crds.sh

generate-models: ## Generate Rust models from CRDs using kopium
	@./scripts/generate-models.sh
