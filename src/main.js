import {
  DEFAULT_LOG_LIMIT,
  mergeLogLines,
  normalizePort,
  portIsValid,
  shouldStickToBottom,
} from "./ui-helpers.js";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const DEFAULTS = {
  serverAddr: "",
  serverPort: 7000,
  token: "",
  localPort: 25565,
  remotePort: 25565,
  frpcPathOverride: null,
};

const STATUS_META = {
  Stopped: {
    label: "Stopped",
    className: "stopped",
    detail: "Ready when you are.",
  },
  Starting: {
    label: "Starting",
    className: "starting",
    detail: "Connecting to your VPS...",
  },
  Running: {
    label: "Running",
    className: "running",
    detail: "Tunnel is live. Share the join address with your friends.",
  },
  Error: {
    label: "Error",
    className: "error",
    detail: "Something went wrong. Check the log and try again.",
  },
};

const elements = {};
let currentStatus = "Stopped";
let pendingLogLines = [];
let renderedLogLines = [];
let logFlushTimer = null;
let saveTimer = null;
let dirtyDraft = false;
let saveInFlight = false;
let saveQueued = false;

function collectSettings() {
  return {
    serverAddr: elements.serverAddr.value.trim(),
    serverPort: normalizePort(elements.serverPort.value, DEFAULTS.serverPort),
    token: elements.token.value,
    localPort: normalizePort(elements.localPort.value, DEFAULTS.localPort),
    remotePort: normalizePort(elements.remotePort.value, DEFAULTS.remotePort),
    frpcPathOverride: elements.frpcPathOverride.value.trim() || null,
  };
}

function hydrateSettings(settings) {
  elements.serverAddr.value = settings.serverAddr ?? DEFAULTS.serverAddr;
  elements.serverPort.value = settings.serverPort ?? DEFAULTS.serverPort;
  elements.token.value = settings.token ?? DEFAULTS.token;
  elements.localPort.value = settings.localPort ?? DEFAULTS.localPort;
  elements.remotePort.value = settings.remotePort ?? DEFAULTS.remotePort;
  elements.frpcPathOverride.value = settings.frpcPathOverride ?? "";
  updateJoinAddress();
  syncFieldValidity();
}

function updateJoinAddress() {
  const host = elements.serverAddr.value.trim() || "your-vps";
  const remotePort = elements.remotePort.value.trim() || DEFAULTS.remotePort;
  elements.joinAddress.textContent = `${host}:${remotePort}`;
}

function queueLog(message, level = "info") {
  const stamp = new Date().toLocaleTimeString();
  pendingLogLines.push(`[${stamp}] [${level.toUpperCase()}] ${message}`);

  if (logFlushTimer !== null) {
    return;
  }

  logFlushTimer = window.setTimeout(flushLogs, 60);
}

function flushLogs() {
  const stickToBottom = shouldStickToBottom(elements.logBox);
  renderedLogLines = mergeLogLines(renderedLogLines, pendingLogLines, DEFAULT_LOG_LIMIT);
  pendingLogLines = [];
  logFlushTimer = null;
  elements.logContent.textContent = renderedLogLines.join("\n");

  if (stickToBottom) {
    elements.logBox.scrollTop = elements.logBox.scrollHeight;
  }
}

function markDirty() {
  dirtyDraft = true;
}

function formIsValid() {
  return (
    elements.serverAddr.value.trim().length > 0 &&
    elements.token.value.trim().length > 0 &&
    portIsValid(elements.serverPort.value) &&
    portIsValid(elements.localPort.value) &&
    portIsValid(elements.remotePort.value)
  );
}

function updateActionState() {
  const isBusy = currentStatus === "Running" || currentStatus === "Starting";
  elements.startButton.disabled = !formIsValid() || isBusy;
  elements.stopButton.disabled = !isBusy;
}

function applyStatus(status, detail) {
  currentStatus = STATUS_META[status] ? status : "Stopped";
  const meta = STATUS_META[currentStatus];
  elements.statusPill.className = `status-pill ${meta.className}`;
  elements.statusText.textContent = meta.label;
  elements.statusDetail.textContent = detail || meta.detail;
  updateActionState();
}

async function persistSettings({ force = false } = {}) {
  if (!dirtyDraft && !force) {
    return;
  }

  if (saveInFlight) {
    saveQueued = true;
    return;
  }

  saveInFlight = true;
  const payload = collectSettings();

  try {
    await invoke("save_settings", {
      settings: payload,
    });
    dirtyDraft = false;
  } catch (error) {
    queueLog(String(error), "error");
  } finally {
    saveInFlight = false;
    if (saveQueued) {
      saveQueued = false;
      await persistSettings({ force: dirtyDraft });
    }
  }
}

function queuePersist() {
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    persistSettings();
  }, 900);
}

function syncFieldValidity() {
  [elements.serverPort, elements.localPort, elements.remotePort].forEach((input) => {
    input.toggleAttribute("aria-invalid", !portIsValid(input.value));
  });
  elements.serverAddr.toggleAttribute("aria-invalid", elements.serverAddr.value.trim().length === 0);
  elements.token.toggleAttribute("aria-invalid", elements.token.value.trim().length === 0);
  updateJoinAddress();
  updateActionState();
}

function applyFrpcState(state) {
  elements.frpcPath.textContent = state.path ?? "Auto-download when you start";
  elements.binaryNote.textContent = state.displayMessage;
}

async function probeFrpc(verbose = false) {
  try {
    const probe = await invoke("probe_frpc", {
      settings: collectSettings(),
    });
    applyFrpcState(probe);
    if (verbose) {
      queueLog(probe.displayMessage);
    }
  } catch (error) {
    elements.binaryNote.textContent = "Could not probe frpc right now.";
    queueLog(String(error), "error");
  }
}

async function handleStart(event) {
  event.preventDefault();
  syncFieldValidity();

  if (!formIsValid()) {
    applyStatus("Error", "Please complete the form with valid ports and a token.");
    queueLog("Start blocked because the form is incomplete or invalid.", "error");
    return;
  }

  applyStatus("Starting", "Connecting to your VPS...");
  await persistSettings({ force: true });

  try {
    const settings = collectSettings();
    await invoke("start_tunnel", { settings });
  } catch (error) {
    applyStatus("Error", String(error));
    queueLog(String(error), "error");
  }
}

async function handleStop() {
  try {
    await invoke("stop_tunnel");
  } catch (error) {
    queueLog(String(error), "error");
  }
}

async function handlePickBinary() {
  try {
    const path = await invoke("pick_frpc_binary");
    if (!path) {
      return;
    }

    elements.frpcPathOverride.value = path;
    markDirty();
    await persistSettings({ force: true });
    queueLog(`Manual frpc selected: ${path}`);
    await probeFrpc();
  } catch (error) {
    queueLog(String(error), "error");
  }
}

async function wireEvents() {
  await listen("tunnel-log", (event) => {
    queueLog(event.payload.message, event.payload.level);
  });

  await listen("tunnel-status", (event) => {
    applyStatus(event.payload.status, event.payload.detail);
  });

  await listen("frpc-download-state", (event) => {
    const payload = event.payload;
    if (payload.path) {
      elements.frpcPath.textContent = payload.path;
    }
    if (payload.message) {
      elements.binaryNote.textContent = payload.message;
    }
    if (payload.stage !== "checking" && payload.stage !== "cached") {
      queueLog(payload.message, payload.stage === "error" ? "error" : "info");
    }
  });
}

function bindInput(input) {
  input.addEventListener("input", () => {
    markDirty();
    syncFieldValidity();
    queuePersist();
  });
}

window.addEventListener("DOMContentLoaded", async () => {
  elements.serverAddr = document.querySelector("#server-addr");
  elements.serverPort = document.querySelector("#server-port");
  elements.token = document.querySelector("#token");
  elements.localPort = document.querySelector("#local-port");
  elements.remotePort = document.querySelector("#remote-port");
  elements.frpcPathOverride = document.querySelector("#frpc-path-override");
  elements.startButton = document.querySelector("#start-button");
  elements.stopButton = document.querySelector("#stop-button");
  elements.pickBinary = document.querySelector("#pick-binary");
  elements.statusPill = document.querySelector("#status-pill");
  elements.statusText = document.querySelector("#status-text");
  elements.statusDetail = document.querySelector("#status-detail");
  elements.joinAddress = document.querySelector("#join-address");
  elements.frpcPath = document.querySelector("#frpc-path");
  elements.binaryNote = document.querySelector("#binary-note");
  elements.logBox = document.querySelector("#log-box");
  elements.logContent = document.querySelector("#log-content");
  elements.form = document.querySelector("#tunnel-form");

  await wireEvents();

  [
    elements.serverAddr,
    elements.serverPort,
    elements.token,
    elements.localPort,
    elements.remotePort,
  ].forEach(bindInput);

  elements.form.addEventListener("submit", handleStart);
  elements.stopButton.addEventListener("click", handleStop);
  elements.pickBinary.addEventListener("click", handlePickBinary);

  window.addEventListener("blur", () => {
    persistSettings();
  });

  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") {
      persistSettings();
    }
  });

  window.addEventListener("beforeunload", () => {
    persistSettings({ force: true });
  });

  try {
    const settings = await invoke("load_settings");
    hydrateSettings(settings);
  } catch (error) {
    queueLog(`Failed to load saved settings: ${error}`, "error");
    hydrateSettings(DEFAULTS);
  }

  applyStatus("Stopped", "Ready when you are.");
  syncFieldValidity();
  await probeFrpc(false);
  queueLog("FlyTunnel is ready.");
});
