# Security Policy

## Reporting a Vulnerability

Please do not report security vulnerabilities in public GitHub issues or public GitHub discussions.

While `patchloom/patchloom` remains private, GitHub private vulnerability reporting is not available yet. Until the repository becomes public, use the same private collaboration path already used to access the repository.

Once the repository becomes public, maintainers should enable GitHub private vulnerability reporting and use it as the default intake path.

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

Once the repository is public, GitHub private vulnerability reporting should become the default private channel for security reports.

It is not meant to become a general-purpose private contact method for product ideas, support, or partnership requests.
