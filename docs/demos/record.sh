#!/usr/bin/env bash
# Record the docs demo videos with VHS (https://github.com/charmbracelet/vhs).
#
# Each docs/demos/<name>.tape scripts one demo; this renders them into
# docs/static/images/demo-<name>.mp4 against the kind dev cluster, so the
# videos can be re-recorded whenever the UI changes:
#
#   just docs-demos            # every tape
#   just docs-demos main       # one tape
#
# Needs: vhs (brew install vhs), Docker, and the flux9s-simple kind cluster
# (./scripts/dev-clusters.sh simple). Recordings never touch your own
# kubeconfig or flux9s config: both are swapped for demo-only copies.
set -euo pipefail
cd "$(dirname "$0")"
DEMOS="$PWD"
ROOT="$(cd ../.. && pwd)"
CLUSTER="flux9s-simple"

command -v vhs >/dev/null || { echo "vhs not found — brew install vhs" >&2; exit 1; }
kind get clusters 2>/dev/null | grep -qx "$CLUSTER" \
  || { echo "kind cluster $CLUSTER not found — ./scripts/dev-clusters.sh simple" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
kind get kubeconfig --name "$CLUSTER" > "$WORK/kubeconfig"
export KUBECONFIG="$WORK/kubeconfig"
export XDG_CONFIG_HOME="$WORK/config"
cp -R "$DEMOS/config" "$XDG_CONFIG_HOME"

echo "▶ Building flux9s (release)"
cargo build --release --quiet --manifest-path "$ROOT/Cargo.toml"
export PATH="$ROOT/target/release:$PATH"

# A known starting state: nothing left suspended or scaled by a previous run.
kubectl -n flux-resources patch helmrelease podinfo --type=merge \
  -p '{"spec":{"suspend":false}}' >/dev/null
kubectl -n flux-resources scale deployment podinfo --replicas=1 >/dev/null
kubectl -n flux-resources rollout status deployment podinfo --timeout=120s >/dev/null
# graph.tape restarts the Flux Operator: start from a settled rollout
kubectl -n flux-system rollout status deployment flux-operator --timeout=120s >/dev/null

tapes=("$@")
[ ${#tapes[@]} -eq 0 ] && tapes=($(ls "$DEMOS"/*.tape | xargs -n1 basename | sed 's/\.tape$//'))
for name in "${tapes[@]}"; do
  echo "▶ Recording $name"
  (cd "$DEMOS" && vhs "$name.tape")
done
echo "✓ Videos written to docs/static/images/"
