#!/usr/bin/env node
/**
 * Fallback launcher when a host starts the MCPB via entry_point instead of
 * mcp_config.command. Prefers a PATH-installed patchloom binary, then npx.
 *
 * Version is stamped by scripts/pack-mcpb.sh from Cargo.toml.
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { version } = require("../package.json");

const npmSpec = `patchloom@${version}`;
const isWin = process.platform === "win32";

function run(command, args) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      stdio: "inherit",
      shell: isWin,
      env: process.env,
    });
    child.on("error", () => resolve(127));
    child.on("exit", (code, signal) => {
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

const pathCode = await run(isWin ? "patchloom.cmd" : "patchloom", [
  "mcp-server",
]);
if (pathCode === 0) process.exit(0);

// Not on PATH (or failed for another reason): install/run via npx.
const npxCode = await run(isWin ? "npx.cmd" : "npx", [
  "-y",
  npmSpec,
  "mcp-server",
]);
process.exit(npxCode);
