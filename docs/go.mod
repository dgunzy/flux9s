module github.com/dgunzy/flux9s/docs

go 1.21

// Hugo theme module, pinned deliberately. Nothing here imports docsy from Go
// code — it is referenced only by `theme = ['github.com/docsy/docsy/theme']`
// in config.toml — so `go mod tidy` (and `hugo mod tidy`) DELETE this line,
// after which Hugo resolves the theme to whatever the latest release happens
// to be. Do not run either in this module (see .github/workflows/docs.yml).
//
// Upgrading: `hugo mod get github.com/docsy/docsy/theme@vX.Y.Z`, then
// `hugo mod npm pack && npm install` (the theme's Bootstrap / Font Awesome
// come from npm), and bump hugo-version in docs.yml to a Hugo release the
// docsy release notes list as supported.
require github.com/docsy/docsy/theme v0.18.0
