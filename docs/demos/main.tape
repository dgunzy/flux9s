# Home page demo: the Flux list, a resource's describe, its graph, the Flux
# Operator's live workload detail, and the pulse dashboard.
# Render with: just docs-demos main
#
# Steps wait for what they expect on screen (`Wait+Screen`), not fixed
# times, so a slow connect or fetch can't knock the script out of step.

Output ../static/images/demo-main.mp4

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
Sleep 2s

# Move through the Flux list
Down Sleep 400ms Down Sleep 400ms Down
Sleep 1s

# Narrow to HelmReleases, then filter to podinfo
Type ":hr" Sleep 700ms Enter
Wait+Screen /HelmRelease \(/
Sleep 1s
Type "/" Sleep 500ms Type "podinfo" Sleep 700ms Enter
Wait+Screen /Showing 1 of/
Sleep 1s

# Describe it
Type "d"
Wait+Screen /Describe - HelmRelease/
Sleep 3s
Escape
Wait+Screen /HelmRelease \(1\)/
Sleep 1s

# Its graph: sources above, what it manages below
Type "g"
Wait+Screen /HELMRELEASE/
Sleep 2.5s
# Focus moves through the nodes
Down
Wait+Screen /Workloads \(1\)/
Sleep 2s
Escape
Wait+Screen /HelmRelease \(1\)/
Sleep 1s

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

# Its live detail: rollout, pods, CPU / memory against requests and limits
Enter
Wait+Screen /Pods \(\d+\)/
Sleep 6s
Escape
Wait+Screen /deployments \(/
Sleep 1s

# The pulse dashboard
Type ":pulse" Sleep 700ms Enter
Wait+Screen /Failures/
Sleep 4s
Type ":q" Enter
Sleep 500ms
