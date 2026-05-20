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
