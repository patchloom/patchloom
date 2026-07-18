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

## Publish to Smithery

```bash
# One-time: smithery auth login  (or set SMITHERY_API_KEY)
make pack-mcpb
smithery mcp publish target/mcpb/patchloom-<version>.mcpb -n patchloom/patchloom
```

CI: `.github/workflows/publish-smithery.yml` packs after a GitHub Release and
publishes when `SMITHERY_API_KEY` is configured (soft-skip otherwise).
