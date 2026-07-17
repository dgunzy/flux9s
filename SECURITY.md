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

Dependency advisories are monitored via `cargo audit` in CI and Dependabot.
