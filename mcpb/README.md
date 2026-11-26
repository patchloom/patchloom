# Patchloom MCPB (Smithery / desktop MCP)

Local stdio MCP Bundle for Patchloom. Hosts that support MCPB (Claude Desktop,
Smithery local install, and others) can install this package so clients run
`patchloom mcp-server` without manual JSON config.

## Build

From the repo root:

```bash
make pack-mcpb
```

Output: `target/mcpb/patchloom-<version>.mcpb`

The pack script stamps `version` from root `Cargo.toml` into `manifest.json`,
`package.json`, and the `npx patchloom@<version>` args.

## Runtime

1. Prefer a `patchloom` binary already on `PATH` (Homebrew, cargo install, etc.).
2. Otherwise run `npx -y patchloom@<version> mcp-server` (Node.js 18+).

This reuses the multi-platform npm installer from cargo-dist instead of shipping
several large native binaries inside the zip.

## Smithery

Smithery stopped server publish on 2026-10-31. Accounts, API keys, and the
publish API no longer accept uploads. [smithery.ai](https://smithery.ai/)
remains a public directory. Do not create `SMITHERY_API_KEY`.

`make pack-mcpb` still builds the bundle for desktop hosts. CI packs it after
a GitHub Release. A leftover key that gets HTTP 403 `servers:write` is a
skip, not a failed release.
