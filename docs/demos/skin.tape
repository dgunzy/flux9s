# User guide demo: the :skin picker previews each theme live as you move
# through it.
# Render with: just docs-demos skin

Output ../static/images/demo-skin.mp4

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

# Open the picker; each step previews that theme on the live list
Type ":skin" Sleep 700ms Enter
Wait+Screen /Select Theme/
Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s
Down Sleep 1.5s

# Esc backs out and restores the original theme
Escape
Sleep 2s
Type ":q" Enter
Sleep 500ms
