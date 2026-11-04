/**
 * E2E launcher: the debug binary serves its UI from the Vite dev server
 * (devUrl), so start `vite` on the fixed port, wait for it, run WDIO, and
 * always tear the server down. Zero extra npm deps.
 */
import { spawn, spawnSync } from "node:child_process";
import net from "node:net";

const PORT = 1420;

// vite may bind IPv6-localhost only ([::1]) on Windows — probe both stacks.
function probe(port, host) {
  return new Promise((resolve) => {
    const sock = net.connect({ port, host });
    sock.once("connect", () => {
      sock.destroy();
      resolve(true);
    });
    sock.once("error", () => {
      sock.destroy();
      resolve(false);
    });
  });
}

async function waitPort(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if ((await probe(port, "127.0.0.1")) || (await probe(port, "::1"))) return;
    if (Date.now() > deadline) throw new Error(`vite not up on :${port}`);
    await new Promise((r) => setTimeout(r, 300));
  }
}

const vite = spawn("pnpm", ["dev"], { stdio: "ignore", shell: true, detached: true });
try {
  await waitPort(PORT, 30_000);
  const res = spawnSync("pnpm", ["exec", "wdio", "run", "e2e/wdio.conf.ts"], {
    stdio: "inherit",
    shell: true,
  });
  process.exitCode = res.status ?? 1;
} catch (e) {
  console.error(String(e));
  process.exitCode = 1;
} finally {
  try {
    process.kill(-vite.pid);
  } catch {
    vite.kill();
  }
}
