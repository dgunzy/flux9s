module github.com/dgunzy/flux9s/docs

go 1.21

// Hugo theme module, pinned deliberately. Nothing here imports docsy from Go
// code — it is referenced only by `theme = ['github.com/google/docsy']` in
// config.toml — so `go mod tidy` DELETES this line, after which Hugo resolves
// the theme to whatever the latest release happens to be. Do not run
// `go mod tidy` in this module (see .github/workflows/docs.yml).
//
// Do not bump to v0.16.0 as-is: that release moved the theme into a nested
// module, so layouts no longer live at the module root and every shortcode
// fails to resolve. Upgrading means switching the path to
// github.com/google/docsy/theme (tagged theme/v0.16.0) and following the
// docsy 0.16.0 upgrade guide (Bootstrap/Font Awesome now come from npm).
require github.com/google/docsy v0.15.0
