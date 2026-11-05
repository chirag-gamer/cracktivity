#!/usr/bin/env node
/**
 * Downloads the pinned llama.cpp build's llama-server.exe into
 * src-tauri/resources/ (D-030: tag b12110 verified — binary-bearing release,
 * llama-server.exe present, flags confirmed). Run before `pnpm tauri build`.
 * The tag is recorded in release notes per D-003's consequence.
 *
 * Extraction always goes through PowerShell's Expand-Archive: shelling out
 * to `unzip` mangles Windows paths whose trailing drive separators interact
 * with shell quoting (seen live: the exe landed in a folder literally named
 * `resources"`).
 */
import { createWriteStream } from "node:fs";
import { mkdir, rm, readdir, copyFile } from "node:fs/promises";
import { pipeline } from "node:stream/promises";
import { execSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TAG = "b12110";
const ZIP_URL = `https://github.com/ggml-org/llama.cpp/releases/download/${TAG}/llama-${TAG}-bin-win-cpu-x64.zip`;
const RES = fileURLToPath(new URL("../src-tauri/resources/", import.meta.url)).replace(
  /[\\/]+$/,
  "",
);

await mkdir(RES, { recursive: true });
console.log("downloading", ZIP_URL);
const res = await fetch(ZIP_URL);
if (!res.ok) throw new Error(`download failed: ${res.status}`);
const tmpZip = path.join(RES, `llama-${TAG}.zip`);
await pipeline(res.body, createWriteStream(tmpZip));

const stage = path.join(RES, "llama-stage");
await rm(stage, { recursive: true, force: true });
execSync(
  `powershell -NoProfile -Command "Expand-Archive -Force '${tmpZip}' '${stage}'"`,
  { stdio: "inherit" },
);
// The zip may nest files under a top-level folder — locate the root that
// holds the binaries. b12110 splits llama-server into a stub exe +
// llama-server-impl.dll with a runtime DLL closure (llama/ggml/libomp), so
// ALL DLLs ship beside the exe (verified: server runs with this set).
const findBinRoot = async (dir) => {
  const names = await readdir(dir);
  if (names.includes("llama-server.exe")) return dir;
  for (const e of await readdir(dir, { withFileTypes: true })) {
    if (e.isDirectory()) {
      const hit = await findBinRoot(path.join(dir, e.name));
      if (hit) return hit;
    }
  }
  return null;
};
const binRoot = await findBinRoot(stage);
if (!binRoot) throw new Error("llama-server.exe not found in the release zip");
for (const f of await readdir(binRoot)) {
  if (f === "llama-server.exe" || f.endsWith(".dll")) {
    await copyFile(path.join(binRoot, f), path.join(RES, f));
  }
}
// The bundler globs resources/*, so the extracted tree must not survive.
await rm(stage, { recursive: true, force: true });
await rm(tmpZip, { force: true });
console.log("sidecar runtime (exe + DLLs) placed in", RES);
