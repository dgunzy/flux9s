.PHONY: help update-flux fetch-crds generate-models clean-models build check test

help: ## Show this help message
	@echo "Flux TUI - Build and Maintenance Commands"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-20s %s\n", $$1, $$2}'

update-flux: ## Fetch CRDs and generate models (full update)
	@./scripts/update-flux.sh

fetch-crds: ## Download Flux CRDs from GitHub releases
	@./scripts/fetch-crds.sh

generate-models: ## Generate Rust models from CRDs using kopium
	@./scripts/generate-models.sh

clean-models: ## Remove generated model files
	@echo "Cleaning generated models..."
	@rm -rf src/models/_generated/*.rs
	@rm -rf crds/*.yaml
	@echo "✓ Cleaned"

build: ## Build the project
	@cargo build

check: ## Check the project (without building)
	@cargo check

test: ## Run tests
	@cargo test

clippy: ## Run clippy linter
	@cargo clippy -- -D warnings

fmt: ## Format code
	@cargo fmt

fmt-check: ## Check formatting without modifying files
	@cargo fmt -- --check

