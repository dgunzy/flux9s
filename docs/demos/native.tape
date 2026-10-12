# Native resources demo (preview): turn it on with :native, browse
# Deployments across namespaces with what manages each, describe one, and
# the warning before editing something Flux manages.
# Render with: just docs-demos native

Output ../static/images/demo-native.mp4

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

# Opt in for this session
Type ":native" Sleep 700ms Enter
Wait+Screen /Native resources on/
Sleep 2s

# Every namespace, then any kind by its kubectl name
Type ":ns all" Sleep 700ms Enter
Sleep 1s
Type ":deploy" Sleep 700ms Enter
Wait+Screen /deployments \(\d+\) \[all namespaces\]/
Wait+Screen /Flux instance/
Sleep 3s
Down Sleep 400ms Down Sleep 400ms Down Sleep 400ms Down
Sleep 1s

# Describe one: Managed By resolves through the owner chain
Type "/" Sleep 500ms Type "podinfo" Sleep 700ms Enter
Wait+Screen /podinfo/
Sleep 800ms
Type "d"
Wait+Screen /Managed By: Flux/
Sleep 3.5s
Escape
Wait+Screen /deployments \(/
Sleep 800ms

# Editing something Flux manages asks first
Type "e"
Wait+Screen /Edit anyway/
Sleep 4s
Type "n"
Wait+Screen /deployments \(/
Sleep 1s

# Pods too
Type ":po" Sleep 700ms Enter
Wait+Screen /pods \(\d+\)/
Sleep 3s

Type ":q" Enter
Sleep 500ms
