#!/usr/bin/env bash
# dev-clusters.sh - Tear down all kind clusters and rebuild two dedicated test clusters
#
# Creates two clearly-named clusters:
#
#   flux9s-simple   — one of every Flux kind; good for day-to-day dev testing
#   flux9s-stress   — 40+ resources of every kind; use to test page-scroll / large lists
#
# Both clusters are installed via the Flux Operator (no flux bootstrap).
# Resources deliberately point to real public sources so some actually reconcile.
#
# Usage:
#   ./scripts/dev-clusters.sh              # build both clusters (default)
#   ./scripts/dev-clusters.sh simple       # build flux9s-simple only
#   ./scripts/dev-clusters.sh stress       # build flux9s-stress only
#   ./scripts/dev-clusters.sh delete       # delete all kind clusters and exit

set -euo pipefail

# ── colours ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "${CYAN}▶ $*${RESET}"; }
success() { echo -e "${GREEN}✓ $*${RESET}"; }
warn()    { echo -e "${YELLOW}! $*${RESET}"; }
die()     { echo -e "${RED}✗ $*${RESET}" >&2; exit 1; }
header()  { echo -e "\n${BOLD}${CYAN}══ $* ══${RESET}\n"; }

# ── constants ──────────────────────────────────────────────────────────────────
SIMPLE_CLUSTER="flux9s-simple"
STRESS_CLUSTER="flux9s-stress"

FLUX_OPERATOR_CHART="oci://ghcr.io/controlplaneio-fluxcd/charts/flux-operator"
FLUX_OPERATOR_VERSION="0.19.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST_DIR="${SCRIPT_DIR}/dev-manifests"

# ── helpers ────────────────────────────────────────────────────────────────────

# Wait for a deployment to be available
wait_deploy() {
    local ns="$1" deploy="$2"
    info "Waiting for $ns/$deploy …"
    kubectl wait deployment/"$deploy" \
        --namespace "$ns" \
        --for=condition=Available \
        --timeout=120s 2>/dev/null || {
        warn "$deploy not ready after 120s – continuing anyway"
    }
}

# Install the Flux Operator via Helm and wait for it to be ready
install_flux_operator() {
    local ctx="$1"
    info "Installing Flux Operator (helm) …"
    kubectl --context "$ctx" create namespace flux-system --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    helm install flux-operator "$FLUX_OPERATOR_CHART" \
        --kube-context "$ctx" \
        --namespace flux-system \
        --version "$FLUX_OPERATOR_VERSION" \
        --wait \
        --timeout 3m \
        2>&1 | grep -E 'deployed|already|Error' || true

    wait_deploy "flux-system" "flux-operator"
    success "Flux Operator ready"
}

# Apply a FluxInstance to install the full Flux distribution
apply_fluxinstance() {
    local ctx="$1"
    info "Applying FluxInstance (installs source/kustomize/helm/notification controllers) …"
    kubectl --context "$ctx" apply -f "${MANIFEST_DIR}/fluxinstance.yaml"

    # Wait for Flux controllers to start
    sleep 5
    for deploy in source-controller kustomize-controller helm-controller notification-controller; do
        wait_deploy "flux-system" "$deploy"
    done
    success "FluxInstance applied – all Flux controllers running"
}

# Wait for all the core Flux CRDs to land before creating resources
wait_for_flux_crds() {
    local ctx="$1"

    info "Waiting for Flux CRDs to be registered …"
    for crd in \
        gitrepositories.source.toolkit.fluxcd.io \
        helmrepositories.source.toolkit.fluxcd.io \
        helmcharts.source.toolkit.fluxcd.io \
        ocirepositories.source.toolkit.fluxcd.io \
        kustomizations.kustomize.toolkit.fluxcd.io \
        helmreleases.helm.toolkit.fluxcd.io \
        alerts.notification.toolkit.fluxcd.io \
        providers.notification.toolkit.fluxcd.io \
        receivers.notification.toolkit.fluxcd.io \
        imagerepositories.image.toolkit.fluxcd.io \
        imagepolicies.image.toolkit.fluxcd.io \
        imageupdateautomations.image.toolkit.fluxcd.io \
        resourcesets.fluxcd.controlplane.io \
        resourcesetinputproviders.fluxcd.controlplane.io; do
        local retries=40
        while ! kubectl --context "$ctx" get crd "$crd" &>/dev/null; do
            retries=$((retries - 1))
            [ $retries -le 0 ] && { warn "Timed out waiting for $crd – skipping"; break; }
            sleep 3
        done
        success "  $crd"
    done
}

# ── manifest apply functions ─────────────────────────────────────────────────

# Apply simple cluster manifests from static YAML files
apply_simple_manifests() {
    local ctx="$1" ns="$2"

    kubectl --context "$ctx" create namespace "$ns" --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    for manifest in "${MANIFEST_DIR}"/simple/*.yaml; do
        info "Applying $(basename "$manifest") …"
        kubectl --context "$ctx" apply -f "$manifest"
    done

    success "Applied all simple cluster manifests (ns=${ns})"
}

# Apply stress cluster manifests from templates with envsubst
apply_stress_manifests() {
    local ctx="$1" ns="$2" suffix="$3"

    kubectl --context "$ctx" create namespace "$ns" --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    for tpl in "${MANIFEST_DIR}"/stress/*.yaml.tpl; do
        NS="$ns" SUFFIX="$suffix" envsubst '${NS} ${SUFFIX}' < "$tpl" \
            | kubectl --context "$ctx" apply -f -
    done

    success "Applied stress manifests (suffix='${suffix}', ns=${ns})"
}

# ── cluster builders ───────────────────────────────────────────────────────────

build_simple_cluster() {
    header "Building  $SIMPLE_CLUSTER  (one of every Flux kind)"

    kind create cluster --name "$SIMPLE_CLUSTER" --wait 60s
    local ctx="kind-${SIMPLE_CLUSTER}"

    install_flux_operator "$ctx"
    apply_fluxinstance "$ctx"
    wait_for_flux_crds "$ctx"

    apply_simple_manifests "$ctx" "flux-resources"

    success "Cluster $SIMPLE_CLUSTER is ready"
    echo
    info "Switch to it:  kubectl config use-context kind-${SIMPLE_CLUSTER}"
    info "Run flux9s:    cargo run"
}

build_stress_cluster() {
    header "Building  $STRESS_CLUSTER  (40+ resources per kind — stress/page-scroll testing)"

    kind create cluster --name "$STRESS_CLUSTER" --wait 60s
    local ctx="kind-${STRESS_CLUSTER}"

    install_flux_operator "$ctx"
    apply_fluxinstance "$ctx"
    wait_for_flux_crds "$ctx"

    # Create resources across multiple namespaces to mirror real-world setups
    # and to get well above two pages (needs ~50+ total resources)
    local namespaces=("team-alpha" "team-beta" "team-gamma" "team-delta")
    local suffixes=("" "-b" "-c" "-d" "-e" "-f" "-g" "-h" "-i" "-j")

    for ns in "${namespaces[@]}"; do
        for suffix in "${suffixes[@]}"; do
            apply_stress_manifests "$ctx" "$ns" "$suffix"
        done
    done

    success "Cluster $STRESS_CLUSTER is ready with many resources across ${#namespaces[@]} namespaces"
    echo
    info "Switch to it:  kubectl config use-context kind-${STRESS_CLUSTER}"
    info "Run flux9s:    cargo run"
    info "Test paging:   press Ctrl+f / Ctrl+b to page through the list"
}

# ── main ───────────────────────────────────────────────────────────────────────

main() {
    local mode="${1:-both}"

    header "flux9s dev cluster setup"
    echo "  Clusters: $SIMPLE_CLUSTER  |  $STRESS_CLUSTER"
    echo "  Mode:     $mode"
    echo

    # ── delete all existing kind clusters ──────────────────────────────────────
    header "Deleting all existing kind clusters"
    existing=$(kind get clusters 2>/dev/null || true)
    if [ -z "$existing" ]; then
        info "No existing clusters to remove"
    else
        while IFS= read -r cluster; do
            [ -z "$cluster" ] && continue
            info "Deleting cluster: $cluster"
            kind delete cluster --name "$cluster"
            success "Deleted: $cluster"
        done <<< "$existing"
    fi

    # ── early exit for delete-only mode ───────────────────────────────────────
    if [ "$mode" = "delete" ]; then
        success "All clusters deleted. Done."
        return 0
    fi

    # ── build requested clusters ───────────────────────────────────────────────
    case "$mode" in
        simple) build_simple_cluster ;;
        stress) build_stress_cluster ;;
        both)
            build_simple_cluster
            build_stress_cluster
            ;;
        *)
            die "Unknown mode '$mode'. Use: both | simple | stress | delete"
            ;;
    esac

    header "All done!"
    echo
    echo "  Available clusters:"
    kind get clusters 2>/dev/null | sed 's/^/    • kind-/'
    echo
    echo "  To test page scrolling in flux9s:"
    echo "    kubectl config use-context kind-${STRESS_CLUSTER}"
    echo "    cargo run"
    echo "    → Press Ctrl+f / Ctrl+b to page down / up through the resource list"
    echo
}

main "${1:-both}"
