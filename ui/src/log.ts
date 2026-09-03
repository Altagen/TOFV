import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { bindChrome } from "./chrome";

type Snapshot = {
  logs: string[];
  command: string | null;
  configRedacted: string | null;
  lastError: string | null;
};

const MAX = 400;
const lines: string[] = [];
let stick = true;

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

let frame = 0;

function paint() {
  // Coalesce: one repaint per frame regardless of incoming log rate.
  if (frame) return;
  frame = requestAnimationFrame(() => {
    frame = 0;
    const el = $("full-log");
    el.textContent = lines.length ? lines.join("\n") : "waiting.";
    if (stick) el.scrollTop = el.scrollHeight;
  });
}

function pushLine(line: string) {
  lines.push(line);
  if (lines.length > MAX) lines.splice(0, lines.length - MAX);
  paint();
}

window.addEventListener("DOMContentLoaded", async () => {
  bindChrome("destroy");

  const logEl = $("full-log");
  logEl.addEventListener("scroll", () => {
    const gap = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight;
    stick = gap < 32;
  });

  try {
    const s = await invoke<Snapshot>("get_state");
    const meta = [s.command, s.configRedacted].filter(Boolean).join("\n\n");
    $("meta").textContent = meta || "—";
    lines.splice(0, lines.length, ...s.logs);
    if (s.lastError && !lines.includes(`tofv: ${s.lastError}`)) {
      lines.push(`tofv: ${s.lastError}`);
    }
    paint();
  } catch (err) {
    $("full-log").textContent = String(err);
  }

  await listen<string>("tofv://log", (ev) => pushLine(ev.payload));
});
