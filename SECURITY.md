# Security Policy

## Reporting a Vulnerability

Please do not report security vulnerabilities in public GitHub issues or public GitHub discussions.

Use [GitHub private vulnerability reporting](https://github.com/patchloom/patchloom/security/advisories/new) to submit security reports. This sends the report directly to the maintainers without public disclosure.

## What To Report Privately

Use a private maintainer contact path for:

- command injection risks
- unsafe file writes or path traversal
- secrets exposure
- unsafe patch application behavior that could cross trust boundaries
- supply chain or release integrity issues
- other vulnerabilities that would create unnecessary risk if disclosed publicly before a fix is ready

## What To Report Publicly

Use public issues for:

- ordinary bugs
- feature requests
- design discussions
- documentation problems
- non-sensitive regressions

## Response Expectations

Maintainers should:

1. acknowledge the report quickly
2. confirm whether the issue is reproducible and in scope
3. work on a fix privately when needed
4. publish a coordinated fix and advisory once it is safe to do so

## Private Contact Scope

GitHub private vulnerability reporting is the default private channel for security reports. It is not meant to become a general-purpose private contact method for product ideas, support, or partnership requests.

## OpenSSF Scorecard notes

Patchloom publishes multi-platform binaries and crates via the cargo-dist
workflow in [`.github/workflows/release.yml`](.github/workflows/release.yml)
and the crates.io publish job. CI also runs `cargo deny` / advisory checks
against the lockfile.

Some Scorecard heuristics do not fully match that posture:

| Check | Observed Scorecard signal | Project reality |
|-------|---------------------------|-----------------|
| **Packaging** | Often `-1` / "packaging workflow not detected" | Releases use cargo-dist (`release.yml`) plus crates.io and Homebrew publishing. Scorecard's packaging detector does not currently recognize this cargo-dist layout; treat a low Packaging score as a known false negative, not absence of packaging. |
| **Vulnerabilities** | May lag OSV/RUSTSEC listings (for example historical `anyhow` advisories) | Authoritative sources for this repo are the checked-in `Cargo.lock` and CI advisory tooling (`cargo deny` / `cargo audit`). If Scorecard still flags an advisory while the lockfile and CI audit are clean (patched versions resolved), prefer the lockfile + CI result. |

Pinned installers in the release workflow (cargo-dist binary and rustup-init
with version + SHA-256) address the Pinned-Dependencies `downloadThenRun`
findings; see issues #1433 and #1436 for history.
