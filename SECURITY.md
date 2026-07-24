# Security Policy

## Supported Versions

Only the latest release of flux9s receives security fixes. Please upgrade to the
newest version before reporting.

## Reporting a Vulnerability

Please **do not** open a public issue for security vulnerabilities.

Report privately via [GitHub private vulnerability reporting](https://github.com/dgunzy/flux9s/security/advisories/new)
(Security tab → "Report a vulnerability").

Include what you can: affected version (`flux9s --version`), a description of the
issue, and reproduction steps. You can expect an acknowledgement within a week.

## Scope notes

flux9s talks to your cluster with the credentials in your kubeconfig and never
sends cluster data anywhere else. Areas of particular interest for reports:

- Anything that causes flux9s to mutate cluster state in read-only mode
- Credential handling around kubeconfig, exec plugins, and proxies
- The release pipeline and published artifacts (crates.io, Homebrew, binstall)

Dependency advisories are monitored via `cargo audit` and `cargo deny` in CI, plus
Dependabot. The build also runs OpenSSF Scorecard, CodeQL, and gitleaks.

## Verifying releases

Every release ships a `SHA256SUMS` file, a cosign signature over it, and SLSA
build-provenance attestations for each archive.

Verify the checksums are authentic (cosign keyless):

```bash
cosign verify-blob \
  --certificate SHA256SUMS.pem \
  --signature SHA256SUMS.sig \
  --certificate-identity-regexp 'https://github.com/dgunzy/flux9s/.+' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
sha256sum -c SHA256SUMS
```

Verify build provenance for a downloaded archive:

```bash
gh attestation verify flux9s-linux-x86_64-musl.tar.gz --repo dgunzy/flux9s
```

A CycloneDX SBOM (`*.cdx.json`) is attached to each release.
