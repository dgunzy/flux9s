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

shopt -s nullglob
dart_sass=(node_modules/sass-embedded-*/dart-sass)
if [ ${#dart_sass[@]} -eq 0 ]; then
  echo "Dart Sass not found — run 'npm ci' in docs/ first" >&2
  exit 1
fi
export PATH="$PWD/${dart_sass[0]}:$PATH"
# The theme module pin in go.mod must not move during a build.
export GOFLAGS=-mod=readonly
exec hugo "$@"
