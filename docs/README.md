# flux9s Documentation Site

This directory contains the Hugo-based documentation site for flux9s, using the [Docsy](https://www.docsy.dev/) theme.

## Local Development

### Prerequisites

- [Hugo Extended](https://gohugo.io/installation/) — the docs workflow pins
  0.166.0, the newest release docsy 0.18 supports (0.160.1 minimum)
- [Go](https://go.dev/doc/install) 1.21 or later (Hugo fetches the docsy theme module)
- [Node.js](https://nodejs.org/) 24 (npm installs Dart Sass, Bootstrap, and Font Awesome)

### Running Locally

1. Install local docs dependencies:

```bash
just docs-deps
```

This installs the npm packages from `package-lock.json` (Dart Sass, PostCSS,
and the theme's Bootstrap / Font Awesome) and downloads the docsy module pinned
in `go.mod`.

2. Fetch metadata (optional, for local testing):

```bash
./scripts/fetch-metadata.sh static/metadata.json
```

3. Start the Hugo server:

```bash
scripts/hugo.sh server
```

`scripts/hugo.sh` runs Hugo with the Dart Sass pinned in `package.json` on
`PATH` (docsy compiles its SCSS with it) and refuses to move the docsy pin.

Or from the repository root after `just docs-deps`:

```bash
just docs-serve
```

The site will be available at `http://localhost:1313/`

### Building

To build the site:

```bash
scripts/hugo.sh --minify
```

Or from the repository root after `just docs-deps`:

```bash
just docs-build
```

The built site will be in the `public/` directory.

## Structure

- `content/` - Markdown content files
- `static/` - Static assets (images, JS, CSS)
- `config.toml` - Hugo configuration
- `go.mod` - Go module for Hugo dependencies
- `scripts/` - Build scripts (metadata fetching, etc.)
- `demos/` - VHS scripts that record the demo videos

## Demo videos

The `demo-*.mp4` videos in `static/images/` are recorded by
[VHS](https://github.com/charmbracelet/vhs), each one from a `demos/<name>.tape`
script, so they can be re-recorded whenever the UI changes:

```bash
brew install vhs
./scripts/dev-clusters.sh simple   # from the repo root, once
just docs-demos                    # every video
just docs-demos main native        # just these ones
```

| Tape | Video | Shown on |
| --- | --- | --- |
| `main.tape` | `demo-main.mp4` | Home page |
| `graph.tape` | `demo-graph.mp4` | User Guide |
| `native.tape` | `demo-native.mp4` | User Guide |
| `skin.tape` | `demo-skin.mp4` | User Guide |

`demos/record.sh` builds flux9s in release mode and resets the podinfo
HelmRelease and Deployment to a known state. It runs with a throwaway
kubeconfig for the `flux9s-simple` kind cluster and the demo-only config in
`demos/config/`, so your own kubeconfig and flux9s settings are never used
or changed.

Each step in a tape waits for what it expects to see (`Wait+Screen /regex/`)
rather than sleeping a fixed time. When a UI change breaks a tape, the
recording stops at the step that no longer matches, so you can see which step
needs updating.

## Deployment

The documentation site is automatically built and deployed to GitHub Pages via the `.github/workflows/docs.yml` workflow when changes are pushed to the `main` branch.

## Metadata

The site fetches project statistics (download counts, GitHub stars, etc.) during the build process using the `scripts/fetch-metadata.sh` script. This metadata is stored in `static/metadata.json` and displayed on the homepage.
