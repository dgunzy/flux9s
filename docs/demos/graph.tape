# Graph and workloads demo: a HelmRelease's graph and its workloads, then
# the Flux Operator's live detail, pod logs, and a rollout restart.
# Render with: just docs-demos graph

Output ../static/images/demo-graph.mp4

Set Shell "bash"
Set FontSize 18
Set Width 1600
Set Height 940
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 150ms
Set CursorBlink false
Set WaitTimeout 60s

Hide
Type "clear" Enter
Show

Type "flux9s" Sleep 700ms Enter
Wait+Screen /Total Resources: [1-9]/
Sleep 1.5s

Type ":hr" Sleep 700ms Enter
Wait+Screen /HelmRelease \(/
Type "/" Sleep 500ms Type "podinfo" Sleep 700ms Enter
Wait+Screen /Showing 1 of/
Sleep 800ms

# The graph: sources above, what the release manages below
Type "g"
Wait+Screen /HELMRELEASE/
Sleep 2.5s
Down
Wait+Screen /Workloads \(1\)/
Sleep 1s
Enter
Wait+Screen /KIND\s+NAME\s+NAMESPACE/
Sleep 1.5s

# Back out of the graph
Escape
Wait+Screen /HELMRELEASE/
Sleep 800ms
Escape
Wait+Screen /HelmRelease \(1\)/
Sleep 800ms

# Native mode reaches any Deployment: the Flux Operator's own, which sets
# requests and limits, so its usage bars have something to measure against
Type ":native" Sleep 700ms Enter
Wait+Screen /Native resources on/
Sleep 1.5s
Type ":ns all" Sleep 700ms Enter
Sleep 1s
Type ":deploy" Sleep 700ms Enter
Wait+Screen /deployments \(\d+\) \[all namespaces\]/
Sleep 1.5s
Type "/" Sleep 500ms Type "flux-operator" Sleep 700ms Enter
Sleep 1.5s

# Workload detail: rollout, pods, live CPU / memory against requests and limits
Enter
Wait+Screen /Pods \(\d+\)/
Sleep 5s

# Its pod's logs, streaming
Type "l"
Wait+Screen /G Follow/
Sleep 4s
Escape
Wait+Screen /Pods \(\d+\)/
Sleep 1s

# Rollout restart: confirm, then watch the new pod replace the old one
Type "r"
Wait+Screen /Restart Deployment/
Sleep 1.5s
Type "y"
Wait+Screen /Pods \(2\)/
Sleep 4s
Wait+Screen /Pods \(1\)/
Sleep 3s

Type ":q" Enter
Sleep 500ms
