#!/bin/bash
set -eo pipefail

# fetch-crds.sh - Download Flux CRDs from GitHub releases
#
# Downloads CRD YAML files from official Flux controller releases.
# Version pinning is managed here for reproducible builds.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRDS_DIR="$PROJECT_ROOT/crds"
MANIFEST_FILE="$PROJECT_ROOT/manifest.json"

# Ensure crds directory exists
mkdir -p "$CRDS_DIR"

# Flux controller versions (pinned for reproducibility)
# Format: controller:version (one per line)
CONTROLLERS="source-controller:v1.7.3
kustomize-controller:v1.7.2
helm-controller:v1.4.3
notification-controller:v1.7.4
image-reflector-controller:v1.0.3
image-automation-controller:v1.0.3
source-watcher:v2.0.2"

# Base URL for Flux releases
BASE_URL="https://github.com/fluxcd"

echo "Fetching Flux CRDs..."
echo ""

# Count controllers for final message
count=0

# Download each CRD file
echo "$CONTROLLERS" | while IFS=':' read -r controller version; do
    url="${BASE_URL}/${controller}/releases/download/${version}/${controller}.crds.yaml"
    output_file="${CRDS_DIR}/${controller}.crds.yaml"
    
    echo "  → ${controller} (${version})"
    if curl -sSLf "$url" -o "$output_file"; then
        count=$((count + 1))
    else
        echo "Error: Failed to download ${controller} CRD" >&2
        exit 1
    fi
done

# Count for final message (need to do separately due to subshell)
count=$(echo "$CONTROLLERS" | wc -l | tr -d ' ')
echo ""
echo "✓ Successfully downloaded ${count} CRD files to ${CRDS_DIR}"

# Create/update manifest.json with version info
{
    echo "{"
    echo "  \"generated_at\": \"$(date -u +"%Y-%m-%dT%H:%M:%SZ")\","
    echo "  \"flux_versions\": {"
    first=true
    echo "$CONTROLLERS" | while IFS=':' read -r controller version; do
        if [ "$first" = true ]; then
            first=false
        else
            echo ","
        fi
        printf "    \"%s\": \"%s\"" "$controller" "$version"
    done
    echo ""
    echo "  }"
    echo "}"
} > "$MANIFEST_FILE"

echo "✓ Updated manifest.json"

