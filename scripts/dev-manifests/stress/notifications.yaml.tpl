apiVersion: notification.toolkit.fluxcd.io/v1beta3
kind: Provider
metadata:
  name: slack${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: slack
  channel: flux-alerts
  address: https://hooks.slack.com/services/EXAMPLE
  secretRef:
    name: slack-token
---
apiVersion: notification.toolkit.fluxcd.io/v1beta3
kind: Provider
metadata:
  name: webhook${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: generic
  address: https://webhook.example.com/flux
---
apiVersion: notification.toolkit.fluxcd.io/v1beta3
kind: Provider
metadata:
  name: grafana${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: grafana
  address: https://grafana.example.com/api/annotations
  secretRef:
    name: grafana-token
---
apiVersion: notification.toolkit.fluxcd.io/v1beta3
kind: Alert
metadata:
  name: on-error${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  providerRef:
    name: slack${SUFFIX}
  eventSeverity: error
  eventSources:
    - kind: Kustomization
      name: "*"
    - kind: HelmRelease
      name: "*"
---
apiVersion: notification.toolkit.fluxcd.io/v1beta3
kind: Alert
metadata:
  name: all-events${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  providerRef:
    name: webhook${SUFFIX}
  eventSeverity: info
  eventSources:
    - kind: GitRepository
      name: "*"
    - kind: Kustomization
      name: "*"
    - kind: HelmRelease
      name: "*"
---
apiVersion: notification.toolkit.fluxcd.io/v1
kind: Receiver
metadata:
  name: github-push${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: github
  events:
    - ping
    - push
  resources:
    - kind: GitRepository
      name: flux-system${SUFFIX}
  secretRef:
    name: webhook-token
---
apiVersion: notification.toolkit.fluxcd.io/v1
kind: Receiver
metadata:
  name: dockerhub${SUFFIX}
  namespace: ${NS}
  labels: {app.kubernetes.io/managed-by: flux9s-dev}
spec:
  type: dockerhub
  events:
    - push
  resources:
    - kind: ImageRepository
      name: podinfo${SUFFIX}
  secretRef:
    name: dockerhub-token
