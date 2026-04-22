#!/usr/bin/env bash
# dev-clusters.sh - Tear down all kind clusters and rebuild dedicated test clusters
#
# Creates three clearly-named clusters:
#
#   flux9s-simple   — Flux 2.8.x (current repo CRD baseline); day-to-day dev testing
#   flux9s-stress   — Flux 2.5.x; 40+ resources of every kind for page-scroll / large lists
#   flux9s-legacy   — Flux 2.2.x with OCIRepository/Bucket at v1beta2; tests the
#                     version-fallback logic (flux9s tries v1 first, then v1beta2)
#
# All clusters are installed via the Flux Operator (no flux bootstrap).
# Resources deliberately point to real public sources so some actually reconcile.
#
# Usage:
#   ./scripts/dev-clusters.sh              # build simple + stress (default, backward-compat)
#   ./scripts/dev-clusters.sh simple       # build flux9s-simple only
#   ./scripts/dev-clusters.sh stress       # build flux9s-stress only
#   ./scripts/dev-clusters.sh legacy       # build flux9s-legacy only
#   ./scripts/dev-clusters.sh all          # build all three clusters
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
LEGACY_CLUSTER="flux9s-legacy"

FLUX_OPERATOR_CHART="oci://ghcr.io/controlplaneio-fluxcd/charts/flux-operator"
FLUX_OPERATOR_VERSION="0.48.0"
DEPLOY_WAIT_TIMEOUT="${FLUX9S_DEV_DEPLOY_WAIT_TIMEOUT:-5s}"
CRD_WAIT_RETRIES="${FLUX9S_DEV_CRD_WAIT_RETRIES:-5}"
CRD_WAIT_SLEEP_SECONDS="${FLUX9S_DEV_CRD_WAIT_SLEEP_SECONDS:-3}"

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
        --timeout "$DEPLOY_WAIT_TIMEOUT" 2>/dev/null || {
        warn "$deploy not ready after $DEPLOY_WAIT_TIMEOUT – continuing anyway"
    }
}

has_crd() {
    local ctx="$1" crd="$2"
    kubectl --context "$ctx" get crd "$crd" &>/dev/null
}

require_crds() {
    local ctx="$1"
    shift
    local missing=0

    for crd in "$@"; do
        if ! has_crd "$ctx" "$crd"; then
            warn "Missing CRD on $ctx: $crd"
            missing=1
        fi
    done

    return $missing
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

# Apply a FluxInstance to install the full Flux distribution.
# $1 = kube context; $2 = FluxInstance file (defaults to fluxinstance.yaml)
apply_fluxinstance() {
    local ctx="$1"
    local manifest="${2:-${MANIFEST_DIR}/fluxinstance.yaml}"
    info "Applying FluxInstance ($(basename "$manifest")) …"
    kubectl --context "$ctx" apply -f "$manifest"

    # Wait for Flux controllers to start
    sleep 5
    for deploy in source-controller kustomize-controller helm-controller notification-controller; do
        wait_deploy "flux-system" "$deploy"
    done
    success "FluxInstance applied – controllers running"
}

# Apply the legacy FluxInstance (Flux 2.2.x, source/kustomize/helm only)
apply_fluxinstance_legacy() {
    local ctx="$1"
    info "Applying legacy FluxInstance (Flux 2.2.x — source/kustomize/helm only) …"
    kubectl --context "$ctx" apply -f "${MANIFEST_DIR}/fluxinstance-legacy.yaml"

    sleep 5
    for deploy in source-controller kustomize-controller helm-controller; do
        wait_deploy "flux-system" "$deploy"
    done
    success "Legacy FluxInstance applied – source/kustomize/helm controllers running"
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
        local retries="$CRD_WAIT_RETRIES"
        while ! kubectl --context "$ctx" get crd "$crd" &>/dev/null; do
            retries=$((retries - 1))
            [ $retries -le 0 ] && { warn "Timed out waiting for $crd – skipping"; break; }
            sleep "$CRD_WAIT_SLEEP_SECONDS"
        done
        success "  $crd"
    done
}

# Wait for the subset of Flux CRDs installed by the legacy cluster
# (source/kustomize/helm only — no notification or image controllers)
wait_for_flux_crds_legacy() {
    local ctx="$1"

    info "Waiting for legacy Flux CRDs to be registered …"
    for crd in \
        gitrepositories.source.toolkit.fluxcd.io \
        helmrepositories.source.toolkit.fluxcd.io \
        helmcharts.source.toolkit.fluxcd.io \
        ocirepositories.source.toolkit.fluxcd.io \
        kustomizations.kustomize.toolkit.fluxcd.io \
        helmreleases.helm.toolkit.fluxcd.io \
        resourcesets.fluxcd.controlplane.io \
        resourcesetinputproviders.fluxcd.controlplane.io; do
        local retries="$CRD_WAIT_RETRIES"
        while ! kubectl --context "$ctx" get crd "$crd" &>/dev/null; do
            retries=$((retries - 1))
            [ $retries -le 0 ] && { warn "Timed out waiting for $crd – skipping"; break; }
            sleep "$CRD_WAIT_SLEEP_SECONDS"
        done
        success "  $crd"
    done
}

# ── manifest apply functions ─────────────────────────────────────────────────

# Apply simple cluster manifests from static YAML files
apply_simple_manifests() {
    local ctx="$1" ns="$2"

    if ! require_crds "$ctx" \
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
        resourcesetinputproviders.fluxcd.controlplane.io; then
        warn "Skipping simple manifests for $ctx because Flux CRDs are not all available yet"
        return 0
    fi

    kubectl --context "$ctx" create namespace "$ns" --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    for manifest in "${MANIFEST_DIR}"/simple/*.yaml; do
        info "Applying $(basename "$manifest") …"
        kubectl --context "$ctx" apply -f "$manifest" || {
            warn "Failed applying $(basename "$manifest") on $ctx – continuing"
        }
    done

    success "Applied all simple cluster manifests (ns=${ns})"
}

# Apply legacy cluster manifests (sources at v1beta2, workloads at v1/v2)
apply_legacy_manifests() {
    local ctx="$1" ns="$2"

    if ! require_crds "$ctx" \
        gitrepositories.source.toolkit.fluxcd.io \
        helmrepositories.source.toolkit.fluxcd.io \
        helmcharts.source.toolkit.fluxcd.io \
        ocirepositories.source.toolkit.fluxcd.io \
        kustomizations.kustomize.toolkit.fluxcd.io \
        helmreleases.helm.toolkit.fluxcd.io \
        resourcesets.fluxcd.controlplane.io \
        resourcesetinputproviders.fluxcd.controlplane.io; then
        warn "Skipping legacy manifests for $ctx because Flux CRDs are not all available yet"
        return 0
    fi

    kubectl --context "$ctx" create namespace "$ns" --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    for manifest in "${MANIFEST_DIR}"/legacy/*.yaml; do
        info "Applying $(basename "$manifest") …"
        kubectl --context "$ctx" apply -f "$manifest" || {
            warn "Failed applying $(basename "$manifest") on $ctx – continuing"
        }
    done

    success "Applied legacy manifests (ns=${ns})"
}

# Apply stress cluster manifests from templates with envsubst
apply_stress_manifests() {
    local ctx="$1" ns="$2" suffix="$3"

    if ! require_crds "$ctx" \
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
        resourcesetinputproviders.fluxcd.controlplane.io; then
        warn "Skipping stress manifests for $ctx because Flux CRDs are not all available yet"
        return 0
    fi

    kubectl --context "$ctx" create namespace "$ns" --dry-run=client -o yaml \
        | kubectl --context "$ctx" apply -f -

    for tpl in "${MANIFEST_DIR}"/stress/*.yaml.tpl; do
        NS="$ns" SUFFIX="$suffix" envsubst '${NS} ${SUFFIX}' < "$tpl" \
            | kubectl --context "$ctx" apply -f - || {
            warn "Failed applying $(basename "$tpl") on $ctx (ns=${ns}, suffix=${suffix}) – continuing"
        }
    done

    success "Applied stress manifests (suffix='${suffix}', ns=${ns})"
}

run_build() {
    local cluster_name="$1"
    shift

    if "$@"; then
        success "Completed build workflow for ${cluster_name}"
        return 0
    fi

    warn "Build workflow failed for ${cluster_name} – continuing with remaining clusters"
    return 0
}

# ── cluster builders ───────────────────────────────────────────────────────────

build_simple_cluster() {
    header "Building  $SIMPLE_CLUSTER  (Flux 2.8.x — current repo CRD baseline)"

    kind create cluster --name "$SIMPLE_CLUSTER" --wait 60s
    local ctx="kind-${SIMPLE_CLUSTER}"

    install_flux_operator "$ctx"
    apply_fluxinstance "$ctx" "${MANIFEST_DIR}/fluxinstance.yaml"
    wait_for_flux_crds "$ctx"

    apply_simple_manifests "$ctx" "flux-resources"

    success "Cluster $SIMPLE_CLUSTER is ready"
    echo
    info "Switch to it:  kubectl config use-context kind-${SIMPLE_CLUSTER}"
    info "Run flux9s:    cargo run"
}

build_stress_cluster() {
    header "Building  $STRESS_CLUSTER  (Flux 2.5.x — stress/page-scroll testing)"

    kind create cluster --name "$STRESS_CLUSTER" --wait 60s
    local ctx="kind-${STRESS_CLUSTER}"

    install_flux_operator "$ctx"
    apply_fluxinstance "$ctx" "${MANIFEST_DIR}/fluxinstance-medium.yaml"
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

build_legacy_cluster() {
    header "Building  $LEGACY_CLUSTER  (Flux 2.2.x — OCIRepository/Bucket at v1beta2)"

    kind create cluster --name "$LEGACY_CLUSTER" --wait 60s
    local ctx="kind-${LEGACY_CLUSTER}"

    install_flux_operator "$ctx"
    apply_fluxinstance_legacy "$ctx"
    wait_for_flux_crds_legacy "$ctx"

    apply_legacy_manifests "$ctx" "flux-resources"

    success "Cluster $LEGACY_CLUSTER is ready"
    echo
    info "Switch to it:  kubectl config use-context kind-${LEGACY_CLUSTER}"
    info "Run flux9s:    cargo run"
    info "Expected:      OCIRepository and Bucket appear via v1beta2 fallback"
    info "               (flux9s tries v1 → 404 → retries as v1beta2 → found)"
    info "               Notification/image resources show 'CRD not available' (expected)"
}

# ── main ───────────────────────────────────────────────────────────────────────

main() {
    local mode="${1:-both}"

    header "flux9s dev cluster setup"
    echo "  Clusters: $SIMPLE_CLUSTER  |  $STRESS_CLUSTER  |  $LEGACY_CLUSTER"
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
        simple) run_build "$SIMPLE_CLUSTER" build_simple_cluster ;;
        stress) run_build "$STRESS_CLUSTER" build_stress_cluster ;;
        legacy) run_build "$LEGACY_CLUSTER" build_legacy_cluster ;;
        both)
            run_build "$SIMPLE_CLUSTER" build_simple_cluster
            run_build "$STRESS_CLUSTER" build_stress_cluster
            ;;
        all)
            run_build "$SIMPLE_CLUSTER" build_simple_cluster
            run_build "$STRESS_CLUSTER" build_stress_cluster
            run_build "$LEGACY_CLUSTER" build_legacy_cluster
            ;;
        *)
            die "Unknown mode '$mode'. Use: both | simple | stress | legacy | all | delete"
            ;;
    esac

    header "All done!"
    echo
    echo "  Available clusters:"
    kind get clusters 2>/dev/null | sed 's/^/    • kind-/'
    echo
    echo "  Day-to-day dev (Flux 2.8.x, current repo CRDs):"
    echo "    kubectl config use-context kind-${SIMPLE_CLUSTER} && cargo run"
    echo
    echo "  Page-scroll stress testing (Flux 2.5.x):"
    echo "    kubectl config use-context kind-${STRESS_CLUSTER} && cargo run"
    echo "    → Press Ctrl+f / Ctrl+b to page through the list"
    echo
    echo "  Version-fallback testing (Flux 2.2.x, OCIRepository/Bucket at v1beta2):"
    echo "    kubectl config use-context kind-${LEGACY_CLUSTER} && cargo run"
    echo "    → OCIRepository and Bucket should appear despite v1 being unavailable"
    echo
}

main "${1:-both}"
