#!/bin/bash
set -eo pipefail

# update-flux.sh - Orchestrate CRD fetching and model generation
#
# This script coordinates the full update process:
# 1. Fetch latest CRDs from Flux releases
# 2. Generate Rust models using kopium
# 3. Verify the build compiles

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔═══════════════════════════════════════════════════════════╗"
echo "║          Flux TUI - Model Update Process                 ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Fetch CRDs
echo "Step 1: Fetching CRDs..."
"${SCRIPT_DIR}/fetch-crds.sh"
echo ""

# Step 2: Generate models
echo "Step 2: Generating Rust models..."
"${SCRIPT_DIR}/generate-models.sh"
echo ""

# Step 3: Verify build
echo "Step 3: Verifying build..."
cd "$PROJECT_ROOT"
if cargo check --quiet 2>&1; then
    echo "✓ Build verification passed"
else
    echo "✗ Build verification failed"
    echo ""
    echo "The generated models may need manual fixes in src/models/extensions.rs"
    exit 1
fi

echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║                    Update Complete!                       ║"
echo "╚═══════════════════════════════════════════════════════════╝"

