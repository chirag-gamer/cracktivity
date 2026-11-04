/** Small display helpers shared by the main window and the notch panel. */

/** Compact relative time: 2m, 5h, Wed, 12 Oct. */
export function relativeTime(ms: number, now = Date.now()): string {
  const diff = Math.max(0, now - ms);
  const m = Math.floor(diff / 60_000);
  if (m < 1) return "now";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = new Date(ms);
  const days = Math.floor(h / 24);
  if (days < 7)
    return d.toLocaleDateString(undefined, { weekday: "short" });
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

import type { Addr } from "./types";

/** Sender display name, falling back to the local part of the address. */
export function senderName(from: Addr | null | undefined): string {
  if (!from) return "(unknown)";
  if (from.name && from.name.trim()) return from.name;
  return from.email.split("@")[0];
}

/** Stable hue from an email address for per-account coloring. */
export function addressHue(email: string): number {
  let h = 0;
  for (let i = 0; i < email.length; i++) h = (h * 31 + email.charCodeAt(i)) % 360;
  return h;
}
