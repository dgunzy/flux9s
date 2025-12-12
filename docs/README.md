# flux9s Documentation Site

This directory contains the Hugo-based documentation site for flux9s, using the [Docsy](https://www.docsy.dev/) theme.

## Local Development

### Prerequisites

- [Hugo Extended](https://gohugo.io/installation/) (version 0.100.0 or later)
- [Go](https://go.dev/doc/install) (version 1.21 or later)

### Running Locally

1. Install Hugo dependencies:

```bash
cd docs
hugo mod get
```

2. Fetch metadata (optional, for local testing):

```bash
./scripts/fetch-metadata.sh static/metadata.json
```

3. Start the Hugo server:

```bash
hugo server
```

The site will be available at `http://localhost:1313/flux9s/`

### Building

To build the site:

```bash
hugo --minify
```

The built site will be in the `public/` directory.

## Structure

- `content/` - Markdown content files
- `static/` - Static assets (images, JS, CSS)
- `config.toml` - Hugo configuration
- `go.mod` - Go module for Hugo dependencies
- `scripts/` - Build scripts (metadata fetching, etc.)

## Deployment

The documentation site is automatically built and deployed to GitHub Pages via the `.github/workflows/docs.yml` workflow when changes are pushed to the `main` branch.

## Metadata

The site fetches project statistics (download counts, GitHub stars, etc.) during the build process using the `scripts/fetch-metadata.sh` script. This metadata is stored in `static/metadata.json` and displayed on the homepage.
