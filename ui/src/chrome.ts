import { getCurrentWindow } from "@tauri-apps/api/window";

type ResizeDir =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

const GRIPS: [string, ResizeDir][] = [
  ["n", "North"],
  ["s", "South"],
  ["e", "East"],
  ["w", "West"],
  ["ne", "NorthEast"],
  ["nw", "NorthWest"],
  ["se", "SouthEast"],
  ["sw", "SouthWest"],
];

export function bindChrome(closeMode: "hide" | "destroy") {
  const win = getCurrentWindow();

  const host = document.createElement("div");
  host.className = "grips";
  host.setAttribute("aria-hidden", "true");
  for (const [cls, dir] of GRIPS) {
    const g = document.createElement("div");
    g.className = `grip ${cls}`;
    g.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      void win.startResizeDragging(dir);
    });
    host.appendChild(g);
  }
  document.body.appendChild(host);

  document.getElementById("win-min")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void win.minimize();
  });
  document.getElementById("win-max")?.addEventListener("click", (e) => {
    e.stopPropagation();
    void win.toggleMaximize();
  });
  document.getElementById("win-close")?.addEventListener("click", (e) => {
    e.stopPropagation();
    if (closeMode === "destroy") void win.close();
    else void win.hide();
  });
  document.querySelector(".chrome-drag")?.addEventListener("dblclick", () => {
    void win.toggleMaximize();
  });
}
