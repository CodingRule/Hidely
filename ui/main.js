const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const list = document.getElementById("windows");
const empty = document.getElementById("empty");
const state = document.getElementById("state");
const summary = document.getElementById("summary");
const status = document.getElementById("status");
const search = document.getElementById("search");
const listTitle = document.getElementById("list-title");
const total = document.getElementById("total");
const rows = new Map(); // hwnd -> { tr, icon, title, app, statusCell, pin, toggle, data }

const FILTERS = {
  all: { title: "All windows", match: () => true, empty: "No other windows are open." },
  pinned: {
    title: "Pinned",
    match: (w) => w.pinned,
    empty: "Nothing is pinned yet. Pin the windows you want the hotkey to hide.",
  },
  hidden: { title: "Hidden", match: (w) => w.hidden, empty: "Nothing is hidden right now." },
};
let filter = "all";

const PIN_ICON =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.500h4l-.600 4 2.100 2.200v1.300h-7V8.700L6.600 6.500 6 2.500ZM8 10v3.800"/></svg>';

function createRow(hwnd) {
  const tr = document.createElement("tr");

  const nameCell = document.createElement("td");
  const name = document.createElement("div");
  name.className = "name";
  let icon = document.createElement("div");
  icon.className = "icon placeholder";
  const title = document.createElement("span");
  title.className = "title";
  name.append(icon, title);
  nameCell.append(name);
  invoke("window_icon", { hwnd }).then((url) => {
    if (!url) return;
    const img = document.createElement("img");
    img.className = "icon";
    img.alt = "";
    img.src = url;
    icon.replaceWith(img);
  });

  const app = document.createElement("td");
  app.className = "app";

  const statusCell = document.createElement("td");
  statusCell.className = "status-cell";

  const actionsCell = document.createElement("td");
  actionsCell.className = "actions-cell";
  const actions = document.createElement("div");
  actions.className = "actions";

  const pin = document.createElement("button");
  pin.type = "button";
  pin.className = "pin";
  pin.innerHTML = PIN_ICON;
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

  actions.append(pin, toggle);
  actionsCell.append(actions);
  tr.append(nameCell, app, statusCell, actionsCell);
  return { tr, title, app, statusCell, pin, toggle, data: null };
}

function updateRow(row, data) {
  const becameHidden = row.data && !row.data.hidden && data.hidden;
  row.data = data;
  row.tr.classList.toggle("is-hidden", data.hidden);
  if (becameHidden) {
    row.tr.classList.remove("just-hidden");
    void row.tr.offsetWidth; // restart the animation
    row.tr.classList.add("just-hidden");
  }
  row.title.textContent = data.title;
  row.title.title = data.title;
  row.app.textContent = data.process;
  row.statusCell.textContent = data.hidden ? "Hidden from capture" : "Visible";
  row.pin.setAttribute("aria-pressed", String(data.pinned));
  row.pin.setAttribute("aria-label", data.pinned ? `Unpin ${data.title}` : `Pin ${data.title}`);
  row.pin.title = data.pinned
    ? "Pinned: the hotkey hides and shows this window"
    : "Pin, so the hotkey hides and shows this window";
  row.toggle.textContent = data.hidden ? "Show" : "Hide";
}

/** Apply the sidebar filter and the search box to the rows already on screen. */
function applyFilter() {
  const query = search.value.trim().toLowerCase();
  const { match, title, empty: emptyText } = FILTERS[filter];
  let shown = 0;
  for (const row of rows.values()) {
    const text = `${row.data.title} ${row.data.process}`.toLowerCase();
    const visible = match(row.data) && (!query || text.includes(query));
    row.tr.hidden = !visible;
    if (visible) shown++;
  }
  listTitle.textContent = title;
  total.textContent = `${shown} ${shown === 1 ? "window" : "windows"}`;
  empty.hidden = shown > 0;
  empty.textContent = query ? `No windows match "${search.value.trim()}".` : emptyText;
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
      list.append(row.tr);
    }
    updateRow(row, data);
  }
  for (const [hwnd, row] of rows) {
    if (!seen.has(hwnd)) {
      row.tr.remove();
      rows.delete(hwnd);
    }
  }
  // Hotkeys are registered during startup, which can finish after the first paint.
  if (!document.getElementById("hotkeys").hasChildNodes()) showHotkeys();

  const hidden = windows.filter((w) => w.hidden).length;
  const pinned = windows.filter((w) => w.pinned).length;
  document.getElementById("count-all").textContent = windows.length;
  document.getElementById("count-pinned").textContent = pinned;
  document.getElementById("count-hidden").textContent = hidden;
  state.classList.toggle("active", hidden > 0);
  summary.textContent =
    hidden === 0 ? "Nothing hidden" : `${hidden} hidden from capture`;
  applyFilter();
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
    for (const key of hotkey.keys.split("+")) {
      const cap = document.createElement("kbd");
      cap.textContent = key;
      keys.append(cap);
    }
    const action = document.createElement("dd");
    action.textContent = hotkey.registered
      ? hotkey.action
      : `${hotkey.action} (unavailable: another app uses this shortcut)`;
    item.classList.toggle("failed", !hotkey.registered);
    item.append(keys, action);
    container.append(item);
  }
}

for (const button of document.querySelectorAll(".filter")) {
  button.addEventListener("click", () => {
    filter = button.dataset.filter;
    for (const other of document.querySelectorAll(".filter")) {
      other.setAttribute("aria-pressed", String(other === button));
    }
    applyFilter();
  });
}
search.addEventListener("input", applyFilter);

// Title bar buttons. Closing sends the window to the tray (handled in Rust).
const appWindow = window.__TAURI__.window?.getCurrentWindow();
document.getElementById("win-min").addEventListener("click", () => appWindow?.minimize());
document.getElementById("win-max").addEventListener("click", () => appWindow?.toggleMaximize());
document.getElementById("win-close").addEventListener("click", () => appWindow?.close());

// A maximised window fills the screen, so it drops the rounded corners and shadow margin.
async function syncMaximized() {
  document.body.classList.toggle("maximized", Boolean(await appWindow?.isMaximized()));
}
appWindow?.onResized(syncMaximized);
syncMaximized();

document.getElementById("show-all").addEventListener("click", () => run(() => invoke("show_all")));
document.addEventListener("contextmenu", (event) => event.preventDefault());
listen("state-changed", refresh);
window.addEventListener("focus", refresh);
setInterval(refresh, 2000);
refresh();
