#!/bin/bash
set -eo pipefail

# fetch-crds.sh - Download Flux CRDs from GitHub releases
#
# Fetches the latest release of each Flux controller by querying the GitHub API.
# Any version can be pinned by setting the corresponding environment variable:
#
#   SOURCE_CONTROLLER_VERSION=v1.7.3 ./scripts/fetch-crds.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CRDS_DIR="$PROJECT_ROOT/crds"

mkdir -p "$CRDS_DIR"

# Fetch the latest release tag for a fluxcd GitHub repo
latest_release() {
    local repo="$1"
    local tag
    tag=$(curl -sSf "https://api.github.com/repos/fluxcd/${repo}/releases/latest" \
        | jq -r '.tag_name')
    if [ -z "$tag" ] || [ "$tag" = "null" ]; then
        echo "Error: Could not fetch latest release for fluxcd/${repo}" >&2
        exit 1
    fi
    echo "$tag"
}

# Download a single controller's CRD file from a GitHub release
fetch_controller_crd() {
    local controller="$1"
    local version="$2"
    local url="https://github.com/fluxcd/${controller}/releases/download/${version}/${controller}.crds.yaml"
    local output="${CRDS_DIR}/${controller}.crds.yaml"

    echo "  → ${controller} (${version})"
    if ! curl -sSLf "$url" -o "$output"; then
        echo "Error: Failed to download ${controller} CRD from ${url}" >&2
        exit 1
    fi
}

# Download a Flux Operator CRD from the main branch
fetch_operator_crd() {
    local name="$1"
    local filename="$2"
    local url="https://raw.githubusercontent.com/controlplaneio-fluxcd/flux-operator/main/config/crd/bases/${filename}"
    local output="${CRDS_DIR}/${name}.crds.yaml"

    echo "  → ${name}"
    if ! curl -sSLf "$url" -o "$output"; then
        echo "Error: Failed to download ${name} CRD from ${url}" >&2
        exit 1
    fi
}

# ---------------------------------------------------------------------------
# Resolve versions — use env var overrides or fetch latest from GitHub API
# ---------------------------------------------------------------------------
echo "Resolving Flux controller versions..."
echo ""

SOURCE_CONTROLLER_VERSION="${SOURCE_CONTROLLER_VERSION:-$(latest_release source-controller)}"
KUSTOMIZE_CONTROLLER_VERSION="${KUSTOMIZE_CONTROLLER_VERSION:-$(latest_release kustomize-controller)}"
HELM_CONTROLLER_VERSION="${HELM_CONTROLLER_VERSION:-$(latest_release helm-controller)}"
NOTIFICATION_CONTROLLER_VERSION="${NOTIFICATION_CONTROLLER_VERSION:-$(latest_release notification-controller)}"
IMAGE_REFLECTOR_CONTROLLER_VERSION="${IMAGE_REFLECTOR_CONTROLLER_VERSION:-$(latest_release image-reflector-controller)}"
IMAGE_AUTOMATION_CONTROLLER_VERSION="${IMAGE_AUTOMATION_CONTROLLER_VERSION:-$(latest_release image-automation-controller)}"
SOURCE_WATCHER_VERSION="${SOURCE_WATCHER_VERSION:-$(latest_release source-watcher)}"

echo "  source-controller:           ${SOURCE_CONTROLLER_VERSION}"
echo "  kustomize-controller:        ${KUSTOMIZE_CONTROLLER_VERSION}"
echo "  helm-controller:             ${HELM_CONTROLLER_VERSION}"
echo "  notification-controller:     ${NOTIFICATION_CONTROLLER_VERSION}"
echo "  image-reflector-controller:  ${IMAGE_REFLECTOR_CONTROLLER_VERSION}"
echo "  image-automation-controller: ${IMAGE_AUTOMATION_CONTROLLER_VERSION}"
echo "  source-watcher:              ${SOURCE_WATCHER_VERSION}"
echo ""

# ---------------------------------------------------------------------------
# Download Flux controller CRDs
# ---------------------------------------------------------------------------
echo "Fetching Flux controller CRDs..."
echo ""

fetch_controller_crd source-controller          "$SOURCE_CONTROLLER_VERSION"
fetch_controller_crd kustomize-controller       "$KUSTOMIZE_CONTROLLER_VERSION"
fetch_controller_crd helm-controller            "$HELM_CONTROLLER_VERSION"
fetch_controller_crd notification-controller    "$NOTIFICATION_CONTROLLER_VERSION"
fetch_controller_crd image-reflector-controller "$IMAGE_REFLECTOR_CONTROLLER_VERSION"
fetch_controller_crd image-automation-controller "$IMAGE_AUTOMATION_CONTROLLER_VERSION"
fetch_controller_crd source-watcher             "$SOURCE_WATCHER_VERSION"

echo ""
echo "✓ Downloaded 7 Flux controller CRD files"

# ---------------------------------------------------------------------------
# Download Flux Operator CRDs (always from main branch)
# ---------------------------------------------------------------------------
echo ""
echo "Fetching Flux Operator CRDs..."
echo ""

fetch_operator_crd flux-operator-resourcesets              "fluxcd.controlplane.io_resourcesets.yaml"
fetch_operator_crd flux-operator-resourcesetinputproviders "fluxcd.controlplane.io_resourcesetinputproviders.yaml"
fetch_operator_crd flux-operator-fluxreports               "fluxcd.controlplane.io_fluxreports.yaml"
fetch_operator_crd flux-operator-fluxinstances             "fluxcd.controlplane.io_fluxinstances.yaml"

echo ""
echo "✓ Downloaded 4 Flux Operator CRD files"

# ---------------------------------------------------------------------------
# Write manifest.json
# ---------------------------------------------------------------------------
cat > "$PROJECT_ROOT/manifest.json" <<EOF
{
  "generated_at": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "flux_versions": {
    "source-controller": "${SOURCE_CONTROLLER_VERSION}",
    "kustomize-controller": "${KUSTOMIZE_CONTROLLER_VERSION}",
    "helm-controller": "${HELM_CONTROLLER_VERSION}",
    "notification-controller": "${NOTIFICATION_CONTROLLER_VERSION}",
    "image-reflector-controller": "${IMAGE_REFLECTOR_CONTROLLER_VERSION}",
    "image-automation-controller": "${IMAGE_AUTOMATION_CONTROLLER_VERSION}",
    "source-watcher": "${SOURCE_WATCHER_VERSION}"
  },
  "flux_operator_crds": [
    "flux-operator-resourcesets",
    "flux-operator-resourcesetinputproviders",
    "flux-operator-fluxreports",
    "flux-operator-fluxinstances"
  ]
}
EOF

echo ""
echo "✓ Updated manifest.json"
echo ""
echo "✓ Total: 11 CRD files downloaded to ${CRDS_DIR}"
