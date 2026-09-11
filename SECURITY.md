# Security Policy

## Supported versions

Security fixes are applied to the latest code on `main`. If tagged releases exist, the most recent release is also considered supported.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report them privately using [GitHub Security Advisories](https://github.com/GunnarKarlsson/android-dashboard/security/advisories/new):

1. Go to the repository’s **Security** tab.
2. Choose **Advisories** → **New draft security advisory** (or use the link above).
3. Include a clear description, steps to reproduce, affected versions if known, and any suggested fix.

We aim to acknowledge reports promptly, typically within a few days. After triage, we will work with you on a fix and coordinated disclosure when appropriate.

## Scope notes

This project shells out to `adb` and may send redacted log summaries to a user-configured AI provider. Reports related to secret leakage, unsafe handling of credentials, or unexpected local command execution are especially welcome. Out of scope: third-party AI provider outages, device OEM bugs, and issues in `adb` itself.
