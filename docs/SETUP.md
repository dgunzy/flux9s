# Documentation Site Setup

This documentation site is built with [Hugo](https://gohugo.io/) and the [Docsy](https://www.docsy.dev/) theme.

## Quick Start

### Prerequisites

- Hugo Extended (v0.100.0+)
- Go 1.21+

### Local Development

```bash
cd docs
hugo mod get  # Install Hugo modules
hugo server   # Start local server
```

Visit `http://localhost:1313/flux9s/`

### Building

```bash
cd docs
hugo --minify
```

Output will be in `docs/public/`

## Structure

```
docs/
├── config.toml          # Hugo configuration
├── go.mod               # Hugo module dependencies
├── content/             # Markdown content
│   ├── _index.md       # Homepage
│   ├── getting-started/
│   ├── user-guide/
│   ├── configuration/
│   └── developer-guide/
├── static/              # Static assets
│   ├── images/         # Screenshots
│   ├── js/             # JavaScript files
│   └── metadata.json   # Generated metadata (build time)
├── layouts/            # Custom layouts/overrides
│   └── partials/
│       └── head-custom.html
└── scripts/            # Build scripts
    └── fetch-metadata.sh
```

## Metadata Fetching

The site automatically fetches project statistics during the GitHub Pages build:

- **Crates.io downloads** - Total download count from crates.io
- **GitHub stars** - Repository star count
- **GitHub releases** - Number of releases

This is handled by `scripts/fetch-metadata.sh` which runs during the build workflow.

## Deployment

The site is automatically deployed to GitHub Pages via `.github/workflows/docs.yml` when changes are pushed to the `main` branch in the `docs/` directory.

## Customization

### Adding Content

Create new markdown files in the appropriate `content/` subdirectory. Use front matter to configure:

```yaml
---
title: "Page Title"
linkTitle: "Short Title"
weight: 10
description: "Page description"
---
```

### Modifying Theme

The Docsy theme is included as a Hugo module. To customize:

1. Override partials in `layouts/partials/`
2. Add custom CSS/JS in `static/`
3. Modify `config.toml` for theme parameters

## Troubleshooting

### Hugo Module Issues

If you encounter module issues:

```bash
cd docs
hugo mod clean
hugo mod get
```

### Metadata Not Loading

The metadata script requires `metadata.json` to be present in `public/` after build. Ensure the GitHub workflow runs `fetch-metadata.sh` before building.
