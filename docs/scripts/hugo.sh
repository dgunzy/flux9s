#!/usr/bin/env bash
# Run Hugo with the Dart Sass pinned in package.json (sass-embedded).
#
# Docsy 0.17+ compiles its SCSS with Dart Sass, which Hugo runs as an
# external `sass --embedded` process. npm installs the native binary under a
# platform-specific package (sass-embedded-<os>-<arch>/dart-sass/sass); put
# that directory first on PATH so local builds and CI use the same version.
# Usage: scripts/hugo.sh [hugo args...]   (run `npm ci` first)
set -euo pipefail
cd "$(dirname "$0")/.."

# On Linux npm installs both the glibc and musl packages, and the musl one
# sorts first but can't run on glibc (e.g. GitHub runners), so use the first
# binary that actually runs rather than the first match.
shopt -s nullglob
dart_sass=""
for dir in node_modules/sass-embedded-*/dart-sass; do
  if "$dir/sass" --version >/dev/null 2>&1; then
    dart_sass="$PWD/$dir"
    break
  fi
done
if [ -z "$dart_sass" ]; then
  echo "No runnable Dart Sass found — run 'npm ci' in docs/ first" >&2
  exit 1
fi
export PATH="$dart_sass:$PATH"
# The theme module pin in go.mod must not move during a build.
export GOFLAGS=-mod=readonly
exec hugo "$@"
