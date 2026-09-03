import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { bindChrome } from "./chrome";

type UiStatus =
  | "idle"
  | "connecting"
  | "up"
  | "disconnecting"
  | "need-cert"
  | "auth-failed"
  | "error";

type Snapshot = {
  status: UiStatus;
  lastError: string | null;
  doctor: { helperOk: boolean; blocking: boolean };
};

const AUTH_RETRY =
  "Code rejected. The token rotates about every 60 s — enter the code shown right now.";

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

function syncGo() {
  const busy = ($("otp") as HTMLInputElement).disabled;
  const otp = ($("otp") as HTMLInputElement).value.trim();
  ($("otp-go") as HTMLButtonElement).disabled = busy || !/^\d{6}$/.test(otp);
}

function setBusy(busy: boolean, label?: string) {
  ($("otp") as HTMLInputElement).disabled = busy;
  ($("otp-cancel") as HTMLButtonElement).disabled = false;
  if (busy) {
    $("otp-error").textContent = label ?? "Connexion…";
    $("otp-error").classList.remove("error");
  } else {
    $("otp-error").classList.add("error");
  }
  syncGo();
}

function resetForm(error?: string | null) {
  const otp = $("otp") as HTMLInputElement;
  otp.disabled = false;
  otp.value = "";
  ($("otp-cancel") as HTMLButtonElement).disabled = false;
  $("otp-error").classList.add("error");
  $("otp-error").textContent = error ?? "";
  $("otp-title").textContent = error ? "New one-time code" : "One-time code";
  $("otp-lede").textContent = error
    ? "The previous code is no longer valid. Read the token again and send the current 6 digits."
    : "Six digits from your token (60 s window). They go into the ephemeral config, never into argv.";
  syncGo();
  otp.focus();
}

async function hidePrompt() {
  await getCurrentWindow().hide();
}

async function send() {
  const otp = ($("otp") as HTMLInputElement).value.trim();
  if (!/^\d{6}$/.test(otp)) {
    $("otp-error").classList.add("error");
    $("otp-error").textContent = "The code must be exactly 6 digits.";
    return;
  }
  setBusy(true);
  try {
    await invoke("connect", { otp });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    setBusy(false);
    $("otp-error").textContent = msg;
    ($("otp") as HTMLInputElement).focus();
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  bindChrome("hide");

  $("otp-cancel").addEventListener("click", async () => {
    const connecting = ($("otp") as HTMLInputElement).disabled;
    if (connecting) {
      try {
        await invoke("disconnect_cmd");
      } catch {
        /* still hide */
      }
    }
    await hidePrompt();
  });
  $("otp-go").addEventListener("click", () => void send());
  $("otp").addEventListener("input", () => {
    const el = $("otp") as HTMLInputElement;
    el.value = el.value.replace(/\D/g, "").slice(0, 6);
    syncGo();
  });
  $("otp").addEventListener("paste", (e) => {
    const text = e.clipboardData?.getData("text") ?? "";
    const digits = text.replace(/\D/g, "").slice(0, 6);
    if (digits) {
      e.preventDefault();
      ($("otp") as HTMLInputElement).value = digits;
      syncGo();
    }
  });
  $("otp").addEventListener("keydown", (e) => {
    if (e.key === "Enter") $("otp-go").click();
  });
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") $("otp-cancel").click();
  });

  await listen<string>("tofv://ask-otp", (ev) => {
    const msg = (ev.payload ?? "").trim();
    resetForm(msg || null);
  });
  await listen<UiStatus>("tofv://status", (ev) => {
    switch (ev.payload) {
      case "connecting":
        setBusy(true);
        break;
      case "up":
        void hidePrompt();
        break;
      case "auth-failed":
        resetForm(AUTH_RETRY);
        break;
      case "need-cert":
        void hidePrompt();
        break;
      case "error":
        void invoke<Snapshot>("get_state").then((s) => {
          setBusy(false);
          $("otp-error").textContent =
            s.lastError ?? "connection failed — check the log.";
        });
        break;
      case "idle":
      case "disconnecting":
        void hidePrompt();
        break;
    }
  });

  try {
    const s = await invoke<Snapshot>("get_state");
    $("otp-helper").hidden = s.doctor.helperOk;
    if (s.status === "connecting") setBusy(true);
    else if (s.status === "auth-failed") resetForm(s.lastError || AUTH_RETRY);
    else if (s.status === "up") await hidePrompt();
    else resetForm(null);
  } catch (e) {
    $("otp-error").textContent = String(e);
  }
});
