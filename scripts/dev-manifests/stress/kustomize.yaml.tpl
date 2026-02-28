apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: infra-controllers${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 10m
  path: ./apps/base/podinfo
  prune: true
  sourceRef:
    kind: GitRepository
    name: flux-system${SUFFIX}
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: infra-configs${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 10m
  path: ./apps/staging
  prune: true
  sourceRef:
    kind: GitRepository
    name: flux-system${SUFFIX}
  dependsOn:
    - name: infra-controllers${SUFFIX}
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: apps${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 10m
  path: ./apps/production
  prune: true
  sourceRef:
    kind: GitRepository
    name: flux-system${SUFFIX}
  dependsOn:
    - name: infra-configs${SUFFIX}
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: terraform-plans${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 30m
  path: ./plans
  prune: false
  sourceRef:
    kind: Bucket
    name: terraform-state${SUFFIX}
