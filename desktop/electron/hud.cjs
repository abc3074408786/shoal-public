// Task HUD window: a small frameless always-on-top monitor for running tasks.
// It renders whatever digest the main window renderer pushes over the
// "velella:hud" channel; it never talks to the runtime itself.
const { BrowserWindow, screen } = require("electron");
const { join, resolve } = require("node:path");
const { mkdirSync, readFileSync, writeFileSync } = require("node:fs");
const { pathToFileURL } = require("node:url");

const HUD_WIDTH = 320;
const DEFAULT_HEIGHT = 460;
const MIN_HEIGHT = 120;
const MAX_HEIGHT = 600;

function createHud({ root, preload, isQuitting }) {
  const entry = pathToFileURL(resolve(__dirname, "../dist/hud.html")).href;
  const boundsFile = join(root, "hud-window.json");
  let saved = null;
  try {
    saved = JSON.parse(readFileSync(boundsFile, "utf8"));
  } catch {}
  let digest = null;
  let height = Math.min(
    MAX_HEIGHT,
    Math.max(MIN_HEIGHT, Number(saved?.height) || DEFAULT_HEIGHT),
  );
  let hud = null, loading = null, loaded = false, showRequested = false;
  let saveTimer;
  const persistBounds = () => {
    if (!hud || hud.isDestroyed()) return;
    try {
      const [x, y] = hud.getPosition();
      const [width, currentHeight] = hud.getSize();
      mkdirSync(root, { recursive: true, mode: 0o700 });
      writeFileSync(boundsFile, JSON.stringify({ x, y, height: currentHeight, width }));
    } catch {}
  };
  const saveBounds = () => {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(persistBounds, 400);
  };
  const hide = () => {
    showRequested = false;
    if (hud && !hud.isDestroyed()) hud.hide();
  };
  const ensureWindow = () => {
    if (hud && !hud.isDestroyed()) return hud;
    const target = new BrowserWindow({
    width: HUD_WIDTH,
    height,
    // Undefined coordinates keep Electron's default centering on first run.
    x: Number.isFinite(saved?.x) ? saved.x : undefined,
    y: Number.isFinite(saved?.y) ? saved.y : undefined,
    frame: false,
    transparent: true,
    resizable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    focusable: false,
    show: false,
    webPreferences: {
      preload,
      backgroundThrottling: false,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
    });
    hud = target;
    target.setAlwaysOnTop(true, "screen-saver");
    target.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    target.webContents.on("will-navigate", (e) => e.preventDefault());
    target.on("close", (e) => {
      if (!isQuitting()) {e.preventDefault();hide();}
      else {clearTimeout(saveTimer);persistBounds();}
    });
    target.on("closed", () => {
      if (hud !== target) return;
      clearTimeout(saveTimer);
      hud = null;loading = null;loaded = false;showRequested = false;
    });
    target.on("moved", saveBounds);
    return target;
  };

  // A saved position may point at a monitor that is no longer attached.
  const clampIntoView = () => {
    const [x, y] = hud.getPosition();
    const area = screen.getDisplayNearestPoint({ x, y }).workArea;
    const [width, height] = hud.getSize();
    const nextX = Math.min(Math.max(x, area.x), area.x + area.width - width);
    const nextY = Math.min(Math.max(y, area.y), area.y + area.height - height);
    if (nextX !== x || nextY !== y) hud.setPosition(nextX, nextY);
  };
  const flush = () => {
    if (digest && loaded && hud && !hud.isDestroyed() && hud.isVisible())
      hud.webContents.send("velella:hud-state", digest);
  };
  const showHud = async () => {
    showRequested = true;
    let target;
    try {
      target = ensureWindow();
      if (!loading) loading = target.loadURL(entry);
      await loading;
      if (hud !== target || target.isDestroyed()) return;
      loaded = true;
      if (!showRequested) return;
      clampIntoView();
      // Never focus the HUD or reveal an unloaded, blank window.
      target.showInactive();
      flush();
    } catch (error) {
      if (!target || hud === target) {
        clearTimeout(saveTimer);
        hud = null;loading = null;loaded = false;showRequested = false;
      }
      if (target && !target.isDestroyed()) target.destroy();
      throw error;
    }
  };

  return {
    get window() {return hud && !hud.isDestroyed() ? hud : null;},
    entry,
    toggle: () => {
      if (showRequested) {hide();return Promise.resolve();}
      return showHud();
    },
    show: showHud,
    hide,
    setDigest(value) {
      digest = value;
      flush();
    },
    flush,
    resize(height) {
      const next = Math.min(
        MAX_HEIGHT,
        Math.max(MIN_HEIGHT, Math.round(Number(height)) || DEFAULT_HEIGHT),
      );
      height = next;
      if (!hud || hud.isDestroyed()) return;
      const [x, y] = hud.getPosition();
      const [width, current] = hud.getSize();
      if (current === next) return;
      hud.setBounds({ x, y, width, height: next });
      saveBounds();
    },
  };
}

module.exports = { createHud };
