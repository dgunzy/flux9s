#!/usr/bin/env bash
# rbac-test.sh — apply a deliberately-restricted RBAC role to an existing kind
# cluster and emit a kubeconfig for it, to exercise flux9s's RBAC "restricted"
# empty-state (#210).
#
# The generated ServiceAccount can list Kustomizations + Sources but NOT
# HelmReleases, Image*, Notification, or Flux Operator kinds — so opening those
# views in flux9s shows the 🔒 restricted message instead of a silent empty list.
#
# Usage:
#   ./scripts/rbac-test.sh [kube-context] [output-kubeconfig]
#   ./scripts/rbac-test.sh                       # defaults: kind-flux9s-simple, /tmp/flux9s-restricted.kubeconfig
#
# Then:
#   KUBECONFIG=/tmp/flux9s-restricted.kubeconfig cargo run
#     :ks  -> Kustomizations list normally
#     :hr  -> HelmRelease view shows "🔒 Restricted — your RBAC can't list HelmRelease ..."
#   Set ui.rbacWarnings=false (flux9s config set ui.rbacWarnings false) to confirm it goes silent.
set -euo pipefail

CTX="${1:-kind-flux9s-simple}"
OUT="${2:-/tmp/flux9s-restricted.kubeconfig}"
NS="flux-system"
SA="flux9s-restricted"
HERE="$(cd "$(dirname "$0")" && pwd)"

echo "▶ Applying restricted RBAC to context '$CTX'..."
kubectl --context "$CTX" apply -f "$HERE/dev-manifests/rbac-restricted.yaml"

echo "▶ Applying per-namespace RBAC scenario (HelmRelease visible in flux-resources, forbidden in restricted-team)..."
kubectl --context "$CTX" apply -f "$HERE/dev-manifests/rbac-namespaced.yaml"

echo "▶ Minting a token for $NS/$SA (TokenRequest API, k8s >= 1.24)..."
TOKEN=$(kubectl --context "$CTX" -n "$NS" create token "$SA" --duration=8h)

SERVER=$(kubectl --context "$CTX" config view --minify -o jsonpath='{.clusters[0].cluster.server}')
CADATA=$(kubectl --context "$CTX" config view --raw --minify -o jsonpath='{.clusters[0].cluster.certificate-authority-data}')

cat > "$OUT" <<EOF
apiVersion: v1
kind: Config
current-context: flux9s-restricted
clusters:
  - name: kind
    cluster:
      server: ${SERVER}
      certificate-authority-data: ${CADATA}
contexts:
  - name: flux9s-restricted
    context:
      cluster: kind
      namespace: ${NS}
      user: flux9s-restricted
users:
  - name: flux9s-restricted
    user:
      token: ${TOKEN}
EOF

echo "✓ Wrote $OUT"
echo
echo "Run flux9s as the restricted user:"
echo "  KUBECONFIG=$OUT cargo run"
echo
echo "  Cluster-wide denial (default flux-system scope, or :ns all):"
echo "    :hr  -> '🔒 Restricted — your RBAC can't list HelmRelease ...' ; :ks lists"
echo
echo "  Per-namespace difference (same kind, different scope):"
echo "    :ns flux-resources ; :hr   -> HelmReleases VISIBLE (podinfo, ingress-nginx)"
echo "    :ns restricted-team ; :ks  -> Kustomization VISIBLE ; :hr -> 🔒 restricted"
echo
echo "  flux9s config set ui.rbacWarnings false   -> confirms the view goes silently empty"
