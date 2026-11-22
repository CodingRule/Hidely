const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const list = document.getElementById("windows");
const empty = document.getElementById("empty");
const summary = document.getElementById("summary");
const status = document.getElementById("status");
const rows = new Map(); // hwnd -> { li, title, detail, pin, toggle, data }

function createRow(hwnd) {
  const li = document.createElement("li");

  const icon = document.createElement("div");
  icon.className = "icon placeholder";
  invoke("window_icon", { hwnd }).then((url) => {
    if (!url) return;
    const img = document.createElement("img");
    img.className = "icon";
    img.alt = "";
    img.src = url;
    icon.replaceWith(img);
  });

  const text = document.createElement("div");
  const title = document.createElement("div");
  title.className = "title";
  const detail = document.createElement("div");
  detail.className = "detail";
  text.append(title, detail);

  const pin = document.createElement("button");
  pin.type = "button";
  pin.textContent = "Pin";
  pin.title = "Pinned windows are hidden and shown together by the hotkey";
  pin.addEventListener("click", async () => {
    const row = rows.get(hwnd);
    await run(() => invoke("set_pinned", { hwnd, pinned: !row.data.pinned }));
  });

  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "toggle";
  toggle.addEventListener("click", async () => {
    const row = rows.get(hwnd);
    toggle.disabled = true;
    await run(() => invoke("set_hidden", { hwnd, hidden: !row.data.hidden }));
    toggle.disabled = false;
  });

  li.append(icon, text, pin, toggle);
  return { li, title, detail, pin, toggle, data: null };
}

function updateRow(row, data) {
  row.data = data;
  row.li.classList.toggle("hidden", data.hidden);
  row.title.textContent = data.title;
  row.title.title = data.title;
  row.detail.textContent = data.hidden ? `${data.process} · hidden from capture` : data.process;
  row.pin.setAttribute("aria-pressed", String(data.pinned));
  row.pin.textContent = data.pinned ? "Pinned" : "Pin";
  row.toggle.textContent = data.hidden ? "Show" : "Hide";
}

async function refresh() {
  const windows = await invoke("windows");
  const seen = new Set();
  for (const data of windows) {
    seen.add(data.hwnd);
    let row = rows.get(data.hwnd);
    if (!row) {
      row = createRow(data.hwnd);
      rows.set(data.hwnd, row);
      list.append(row.li);
    }
    updateRow(row, data);
  }
  for (const [hwnd, row] of rows) {
    if (!seen.has(hwnd)) {
      row.li.remove();
      rows.delete(hwnd);
    }
  }
  // Hotkeys are registered during startup, which can finish after the first paint.
  if (!document.getElementById("hotkeys").hasChildNodes()) showHotkeys();
  empty.hidden = windows.length > 0;
  const hidden = windows.filter((w) => w.hidden).length;
  summary.textContent =
    hidden === 0
      ? "Nothing is hidden."
      : `${hidden} window${hidden === 1 ? " is" : "s are"} hidden from screen capture.`;
}

/** Run an action, show its error (if any), then re-read the real state. */
async function run(action) {
  status.textContent = "";
  try {
    await action();
  } catch (error) {
    status.textContent = `Couldn't do that: ${error}`;
  }
  await refresh();
}

async function showHotkeys() {
  const container = document.getElementById("hotkeys");
  const hotkeys = await invoke("hotkeys");
  container.replaceChildren();
  for (const hotkey of hotkeys) {
    const item = document.createElement("div");
    const keys = document.createElement("dt");
    keys.textContent = hotkey.keys;
    const action = document.createElement("dd");
    action.textContent = hotkey.registered
      ? hotkey.action
      : `${hotkey.action} (unavailable: another app uses this shortcut)`;
    item.classList.toggle("failed", !hotkey.registered);
    item.append(keys, action);
    container.append(item);
  }
}

document.getElementById("show-all").addEventListener("click", () => run(() => invoke("show_all")));
document.addEventListener("contextmenu", (event) => event.preventDefault());
listen("state-changed", refresh);
window.addEventListener("focus", refresh);
setInterval(refresh, 2000);
refresh();
