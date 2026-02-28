apiVersion: source.toolkit.fluxcd.io/v1
kind: HelmChart
metadata:
  name: podinfo${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 1h
  chart: podinfo
  version: ">=6.0.0"
  sourceRef:
    kind: HelmRepository
    name: podinfo${SUFFIX}
---
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: podinfo${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 5m
  chart:
    spec:
      chart: podinfo
      version: ">=6.0.0"
      sourceRef:
        kind: HelmRepository
        name: podinfo${SUFFIX}
  values:
    replicaCount: 1
---
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: ingress-nginx${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  interval: 15m
  chart:
    spec:
      chart: nginx
      version: ">=18.0.0 <19.0.0"
      sourceRef:
        kind: HelmRepository
        name: bitnami${SUFFIX}
  values:
    service:
      type: ClusterIP
