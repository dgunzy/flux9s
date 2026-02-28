apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImageRepository
metadata:
  name: podinfo${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 5m
  image: ghcr.io/stefanprodan/podinfo
---
apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImagePolicy
metadata:
  name: podinfo${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  imageRepositoryRef:
    name: podinfo${SUFFIX}
  policy:
    semver:
      range: ">=6.0.0"
---
apiVersion: image.toolkit.fluxcd.io/v1beta2
kind: ImageUpdateAutomation
metadata:
  name: podinfo${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 30m
  sourceRef:
    kind: GitRepository
    name: flux-system${SUFFIX}
  git:
    checkout:
      ref:
        branch: main
    commit:
      author:
        name: Flux Bot
        email: flux@example.com
