#!/usr/bin/env node
/**
 * Downloads the pinned llama.cpp build's llama-server.exe into
 * src-tauri/resources/ (D-030: tag b12110 verified — binary-bearing release,
 * llama-server.exe present, flags confirmed). Run before `pnpm tauri build`.
 * The tag is recorded in release notes per D-003's consequence.
 */
import { createWriteStream } from "node:fs";
import { mkdir, rm } from "node:fs/promises";
import { pipeline } from "node:stream/promises";
import { execSync } from "node:child_process";

const TAG = "b12110";
const URL = `https://github.com/ggml-org/llama.cpp/releases/download/${TAG}/llama-${TAG}-bin-win-cpu-x64.zip`;
const RES = new URL("../src-tauri/resources/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");

await mkdir(RES, { recursive: true });
console.log("downloading", URL);
const res = await fetch(URL);
if (!res.ok) throw new Error(`download failed: ${res.status}`);
const tmpZip = RES + `llama-${TAG}.zip`;
await pipeline(res.body, createWriteStream(tmpZip));
try {
  execSync(`unzip -o -j "${tmpZip}" llama-server.exe -d "${RES}"`, { stdio: "inherit" });
} catch {
  // Stock Windows has no unzip; PowerShell Expand-Archive extracts all and
  // we keep only llama-server.exe.
  const stage = RES + "llama-stage";
  execSync(
    `powershell -NoProfile -Command "Expand-Archive -Force '${tmpZip}' '${stage}'"`,
    { stdio: "inherit" },
  );
  execSync(
    `powershell -NoProfile -Command "Move-Item -Force '${stage}\llama-server.exe' '${RES}'"`,
    { stdio: "inherit" },
  );
  await rm(stage, { recursive: true, force: true });
}
await rm(tmpZip, { force: true });
console.log("llama-server.exe placed in", RES);
