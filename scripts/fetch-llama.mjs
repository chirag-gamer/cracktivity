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
const extractViaPowershell = async (zip, stage) => {
  // Stock Windows has no unzip; Expand-Archive extracts the whole tree and
  // we locate llama-server.exe wherever it sits (the zip may nest it).
  await rm(stage, { recursive: true, force: true });
  execSync(
    `powershell -NoProfile -Command "Expand-Archive -Force '${zip}' '${stage}'"`,
    { stdio: "inherit" },
  );
  const { readdir } = await import("node:fs/promises");
  const find = async (dir) => {
    for (const e of await readdir(dir, { withFileTypes: true })) {
      const p = `${dir}\${e.name}`;
      if (e.isDirectory()) {
        const hit = await find(p);
        if (hit) return hit;
      } else if (e.name === "llama-server.exe") {
        return p;
      }
    }
    return null;
  };
  const exe = await find(stage);
  if (!exe) throw new Error("llama-server.exe not found in the release zip");
  const { copyFile } = await import("node:fs/promises");
  await copyFile(exe, RES + "llama-server.exe");
};

try {
  execSync(`unzip -o -j "${tmpZip}" llama-server.exe -d "${RES}"`, { stdio: "inherit" });
} catch {
  await extractViaPowershell(tmpZip, RES + "llama-stage");
}
await rm(tmpZip, { force: true });
console.log("llama-server.exe placed in", RES);
