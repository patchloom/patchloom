#!/usr/bin/env node
/**
 * Fallback launcher when a host starts the MCPB via entry_point instead of
 * mcp_config.command. Prefers a PATH-installed patchloom binary, then npx.
 *
 * Version is stamped by scripts/pack-mcpb.sh from Cargo.toml.
 *
 * Only falls through to npx when the binary is not on PATH. After a
 * successful spawn, non-zero exit or host signal must not start a second
 * server via npx (previous bug: any non-zero path exit fell through).
 */
import { spawn, execFileSync } from "node:child_process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { version } = require("../package.json");

const npmSpec = `patchloom@${version}`;
const isWin = process.platform === "win32";

function binaryOnPath(name) {
  try {
    if (isWin) {
      execFileSync("where.exe", [name], { stdio: "ignore" });
    } else {
      execFileSync("which", [name], { stdio: "ignore" });
    }
    return true;
  } catch {
    return false;
  }
}

/**
 * @param {string} command
 * @param {string[]} args
 * @returns {Promise<number>}
 */
function run(command, args) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      stdio: "inherit",
      // Windows needs shell for .cmd shims (npx.cmd, package shims).
      shell: isWin,
      env: process.env,
      windowsHide: true,
    });
    child.on("error", () => resolve(1));
    child.on("exit", (code, signal) => {
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

if (binaryOnPath("patchloom")) {
  process.exit(await run("patchloom", ["mcp-server"]));
}

process.exit(
  await run(isWin ? "npx.cmd" : "npx", ["-y", npmSpec, "mcp-server"]),
);
