/**
 * WDIO + @wdio/tauri-service (D-018 L5). External tauri-driver provider:
 * the production binary stays free of the embedded-driver plugin.
 *
 * The debug binary loads its frontend from the Vite dev server
 * (tauri.conf.json devUrl), so `e2e/run.mjs` starts `pnpm dev` first.
 * `--e2e-seed` seeds the encrypted store with deterministic demo mail
 * (no network): V-3.1's "add mocked account" leg.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));

export const config = {
  runner: "local",
  maxInstances: 1,
  specs: [path.join(root, "specs", "*.e2e.ts")],
  services: [
    [
      "@wdio/tauri-service",
      {
        // Explicit per the service's version-dependent default warning.
        driverProvider: "external",
        appBinaryPath: path.join(root, "..", "src-tauri", "target", "debug", "cracktivity.exe"),
        appArgs: ["--e2e-seed"],
        autoDownloadEdgeDriver: true,
        autoInstallTauriDriver: true,
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: path.join(root, "..", "src-tauri", "target", "debug", "cracktivity.exe"),
        args: ["--e2e-seed"],
      },
    },
  ],
  logLevel: "warn",
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 60_000 },
  reporters: ["spec"],
};
