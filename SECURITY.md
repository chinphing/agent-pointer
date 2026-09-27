# Security Policy

## Supported versions

Security fixes land on the current `main` branch and the latest tagged release.
Older tags are not guaranteed to receive backports.

## Reporting a vulnerability

Do **not** open a public issue for security reports.

1. Use GitHub **Private vulnerability reporting** on this repository, or
2. Email `starphinliu@gmail.com` with a description, impact, and reproduction
   notes that do not include exploit payloads.

You should receive an acknowledgement within a few business days. Please give
maintainers time to assess and ship a fix before public disclosure.

## Known desktop surface

The Tauri desktop shell currently ships with `csp` unset and an asset protocol
scope that includes the user home directory. Treat the desktop app as a local
agent with filesystem and terminal access. Tightening CSP is a follow-up, not
a blocker for source publication.

## Secrets

Never commit Apple signing files, license private keys, or production
`hmac_secret` values. Official signing secrets belong in GitHub Actions
environments, not in this repository.
