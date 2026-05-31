# Contributing to flux9s

Thank you for your interest in contributing!

## DCO Sign-off

All commits must be signed off with the [Developer Certificate of Origin (DCO)](https://developercertificate.org/). Add `-s` to your commit command:

```sh
git commit -s -m "your commit message"
```

This adds a `Signed-off-by` trailer to the commit. The DCO check will fail on PRs without it.

## Development

See the [Developer Guide](DEVELOPER_GUIDE.md) for setup and architecture details.

**Before submitting a PR, run:**

```sh
just ci
```

This runs formatting, linting, and tests — the same checks CI runs.

## Submitting a Pull Request

1. Fork the repo and create a branch from `main`
2. Make your changes with signed-off commits
3. Ensure `just ci` passes locally
4. Open a PR targeting `main`

PRs require one approving review and all CI checks to pass before merging.
