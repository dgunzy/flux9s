apiVersion: fluxcd.controlplane.io/v1
kind: ResourceSetInputProvider
metadata:
  name: github-branches${SUFFIX}
  namespace: ${NS}
  annotations:
    fluxcd.controlplane.io/reconcileEvery: "30s"
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: GitHubBranch
  url: https://github.com/fluxcd/flux2
  secretRef:
    name: github-token
---
apiVersion: fluxcd.controlplane.io/v1
kind: ResourceSetInputProvider
metadata:
  name: cluster-inventory${SUFFIX}
  namespace: ${NS}
  annotations:
    fluxcd.controlplane.io/reconcileEvery: "1m"
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: GitLabBranch
  url: https://gitlab.com/fluxcd/flux2
  secretRef:
    name: gitlab-token
---
apiVersion: fluxcd.controlplane.io/v1
kind: ResourceSet
metadata:
  name: per-branch-envs${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  inputsFrom:
    - kind: ResourceSetInputProvider
      name: github-branches${SUFFIX}
  resources:
    - apiVersion: v1
      kind: ConfigMap
      metadata:
        name: branch-${{ inputs.branch }}${SUFFIX}
        namespace: ${NS}
      data:
        branch: ${{ inputs.branch }}
---
apiVersion: fluxcd.controlplane.io/v1
kind: ResourceSet
metadata:
  name: cluster-configs${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  inputsFrom:
    - kind: ResourceSetInputProvider
      name: github-branches${SUFFIX}
    - kind: ResourceSetInputProvider
      name: cluster-inventory${SUFFIX}
  resources:
    - apiVersion: v1
      kind: ConfigMap
      metadata:
        name: cluster-${{ inputs.region }}${SUFFIX}
        namespace: ${NS}
      data:
        region: ${{ inputs.region }}
