const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const list = $("windows");
const search = $("search");
const errorLine = $("status");
const rows = new Map(); // hwnd -> row parts
let model = null; // the last snapshot from the backend, the single source of truth
let filter = "all";

/** How each status is shown: a distinct shape and words, never colour alone. */
const STATUS = {
  unprotected: { label: "Unprotected", icon: '<circle cx="8" cy="8" r="5.500"/>' },
  applying: {
    label: "Applying…",
    icon: '<path d="M8 2.500a5.500 5.500 0 1 0 5.500 5.500"/>',
  },
  applied: {
    label: "Protection applied",
    icon: '<circle cx="8" cy="8" r="5.500"/><path d="m5.600 8.200 1.700 1.700 3.200-3.500"/>',
  },
  unsupported: {
    label: "Unsupported",
    icon: '<circle cx="8" cy="8" r="5.500"/><path d="m4.200 11.800 7.600-7.600"/>',
  },
  error: { label: "Error", icon: '<path d="M8 2.200 14.200 13H1.800L8 2.200Z"/><path d="M8 6.500v3M8 11.300v.100"/>' },
};

const FILTERS = {
  all: { title: "All windows", match: () => true, empty: "No other windows are open." },
  pinned: {
    title: "Pinned",
    match: (w) => w.pinned,
    empty: "Nothing is pinned. Pin windows to protect them when a session starts.",
  },
  protected: {
    title: "Protected",
    match: (w) => w.status === "applied",
    empty: "No window is protected right now.",
  },
  problems: {
    title: "Needs attention",
    match: (w) => w.status === "error" || w.status === "unsupported",
    empty: "Nothing needs attention.",
  },
};

const PIN_ICON =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.500h4l-.600 4 2.100 2.200v1.300h-7V8.700l2.100-2.200-.600-4ZM8 10v3.800"/></svg>';

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;
const showKeys = (keys) => keys.replaceAll("Super", "Win");

/** Run an action, show its error (if any), then re-read the real state. */
async function run(action) {
  errorLine.textContent = "";
  try {
    await action();
  } catch (error) {
    errorLine.textContent = String(error);
  }
  await refresh();
}

// ------------------------------------------------------------------ window list

function createRow(hwnd) {
  const tr = el("tr");

  const nameCell = el("td");
  const name = el("div", "name");
  const icon = el("div", "icon placeholder");
  const title = el("span", "title");
  name.append(icon, title);
  nameCell.append(name);
  invoke("window_icon", { hwnd }).then((url) => {
    if (!url) return;
    const img = el("img", "icon");
    img.alt = "";
    img.src = url;
    icon.replaceWith(img);
  });

  const app = el("td", "app");

  const statusCell = el("td", "status-cell");
  const badge = el("div", "badge");
  const badgeIcon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  badgeIcon.setAttribute("viewBox", "0 0 16 16");
  badgeIcon.setAttribute("aria-hidden", "true");
  const badgeLabel = el("span");
  badge.append(badgeIcon, badgeLabel);
  const detail = el("div", "detail");
  statusCell.append(badge, detail);

  const actionsCell = el("td", "actions-cell");
  const actions = el("div", "actions");
  const pin = el("button", "pin");
  pin.type = "button";
  pin.innerHTML = PIN_ICON;
  pin.addEventListener("click", () => {
    const row = rows.get(hwnd);
    run(() => invoke("set_pinned", { hwnd, pinned: !row.data.pinned }));
  });
  const toggle = el("button", "toggle");
  toggle.type = "button";
  toggle.addEventListener("click", () => {
    const row = rows.get(hwnd);
    run(() => invoke("set_protected", { hwnd, protected: !row.data.owned }));
  });
  actions.append(pin, toggle);
  actionsCell.append(actions);

  tr.append(nameCell, app, statusCell, actionsCell);
  return { tr, title, app, badge, badgeIcon, badgeLabel, detail, pin, toggle, data: null };
}

function updateRow(row, data) {
  row.data = data;
  const status = STATUS[data.status];
  row.tr.dataset.status = data.status;
  row.title.textContent = data.title;
  row.title.title = data.title;
  row.app.textContent = data.process;

  row.badgeIcon.innerHTML = status.icon;
  row.badgeLabel.textContent = status.label;
  row.badge.title = data.detail;
  // Problems, and protection Hidely did not apply itself, are spelled out under the badge.
  const explain =
    data.status === "error" || data.status === "unsupported" || (data.status === "applied" && !data.owned);
  row.detail.textContent = explain ? data.detail : "";
  row.detail.hidden = !explain;

  row.pin.setAttribute("aria-pressed", String(data.pinned));
  row.pin.setAttribute("aria-label", data.pinned ? `Unpin ${data.title}` : `Pin ${data.title}`);
  row.pin.title = data.pinned
    ? "Pinned: protected when a session starts"
    : "Pin, to protect this window when a session starts";

  const busy = data.status === "applying";
  const byApp = data.status === "applied" && !data.owned;
  row.toggle.textContent = byApp ? "By app" : data.owned ? "Unprotect" : data.status === "error" ? "Retry" : "Protect";
  row.toggle.disabled = busy || byApp;
  row.toggle.title = byApp
    ? "This app protects this window itself, so there is nothing for Hidely to do"
    : data.owned
      ? "Make this window capturable again"
      : "Exclude this window from screen capture";
}

function applyFilter() {
  const query = search.value.trim().toLowerCase();
  const { match, title, empty } = FILTERS[filter];
  let shown = 0;
  for (const row of rows.values()) {
    const text = `${row.data.title} ${row.data.process}`.toLowerCase();
    const visible = match(row.data) && (!query || text.includes(query));
    row.tr.hidden = !visible;
    if (visible) shown++;
  }
  $("list-title").textContent = title;
  $("total").textContent = plural(shown, "window");
  $("empty").hidden = shown > 0;
  $("empty").textContent = query ? `No windows match "${search.value.trim()}".` : empty;
}

// --------------------------------------------------------------------- render

function render(snapshot) {
  model = snapshot;
  const { windows, summary } = snapshot;

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

  const count = (test) => windows.filter(test).length;
  $("count-all").textContent = windows.length;
  $("count-pinned").textContent = count(FILTERS.pinned.match);
  $("count-protected").textContent = count(FILTERS.protected.match);
  $("count-problems").textContent = count(FILTERS.problems.match);
  applyFilter();

  renderSession(summary, snapshot.last_end);
  renderProfiles(snapshot);
  renderHotkeys(snapshot.shortcuts);
  renderNotices(snapshot.notices);

  // Offered only when it would help: Hidely is not elevated yet.
  const restart = $("restart-elevated");
  restart.hidden = snapshot.elevated;
  restart.title = "Needed to protect windows of apps that run as administrator. Ends the current session first.";
}

function renderSession(summary, lastEnd) {
  const problems = summary.errors + summary.unsupported;
  const state = $("state");
  state.classList.toggle("active", summary.session_active);
  state.classList.toggle("attention", problems > 0);

  const button = $("session-button");
  if (!summary.session_active) {
    $("summary").textContent = problems > 0 ? `No session · ${problems} need attention` : "No privacy session";
    $("session-title").textContent = "No privacy session";
    $("session-detail").textContent = lastEnd
      ? lastEnd.failed > 0
        ? `Last session ended: ${plural(lastEnd.restored, "window")} capturable again, ${lastEnd.failed} could not be restored and may still be excluded from capture.`
        : `Last session ended: ${plural(lastEnd.restored, "window")} capturable again.`
      : "Nothing is protected. Pick a profile or pin windows, then start.";
    button.textContent = "Start session";
    button.classList.add("primary");
    return;
  }

  const started = new Date(summary.started_unix * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
  const parts = [`${summary.applied} protected`];
  if (summary.applying) parts.push(`${summary.applying} applying`);
  if (summary.unsupported) parts.push(`${summary.unsupported} unsupported`);
  if (summary.errors) parts.push(plural(summary.errors, "error"));
  $("summary").textContent = `Session on · ${parts.join(" · ")}`;
  $("session-title").textContent = summary.profile_name
    ? `Privacy session on · ${summary.profile_name}`
    : "Privacy session on";
  const partial = problems > 0 && summary.applied > 0 ? " Only some of your windows are protected." : "";
  const watching = summary.watching
    ? " New windows of this profile's apps are protected as they open."
    : " Only the windows you choose are protected.";
  $("session-detail").textContent = `Started ${started}. ${parts.join(", ")}.${partial}${watching}`;
  button.textContent = "End session";
  button.classList.remove("primary");
}

function renderProfiles({ profiles, selected_profile: selected }) {
  const container = $("profiles");
  const options = [{ id: null, name: "No profile" }, ...profiles];
  container.replaceChildren(
    ...options.map((profile) => {
      const row = el("div", "profile");
      const choose = el("button", "profile-choose");
      choose.type = "button";
      choose.setAttribute("role", "radio");
      choose.setAttribute("aria-checked", String(profile.id === selected));
      choose.append(el("span", "radio"), el("span", "profile-name", profile.name));
      if (profile.rules) choose.append(el("span", "count", String(profile.rules.length)));
      choose.title = profile.rules ? `${plural(profile.rules.length, "app")} in this profile` : "Use only pinned windows";
      choose.addEventListener("click", () => run(() => invoke("select_profile", { id: profile.id })));
      row.append(choose);
      if (profile.id) {
        const edit = el("button", "link", "Edit");
        edit.type = "button";
        edit.setAttribute("aria-label", `Edit profile ${profile.name}`);
        edit.addEventListener("click", () => openProfile(profile));
        row.append(edit);
      }
      return row;
    }),
  );
}

function renderHotkeys(shortcuts) {
  $("hotkeys").replaceChildren(
    ...shortcuts.map((shortcut) => {
      const item = el("div");
      const keys = el("dt");
      for (const key of showKeys(shortcut.keys).split("+")) keys.append(el("kbd", "", key));
      const text = shortcut.registered
        ? shortcut.label
        : `${shortcut.label} (not working: another app uses this shortcut)`;
      item.classList.toggle("failed", !shortcut.registered);
      item.append(keys, el("dd", "", text));
      return item;
    }),
  );
}

function renderNotices(notices) {
  $("notices").hidden = notices.length === 0;
  $("notice-text").replaceChildren(...notices.map((notice) => el("p", "", notice)));
}

async function refresh() {
  render(await invoke("snapshot"));
}

// --------------------------------------------------------------- session start

async function openStart() {
  const preview = await invoke("session_preview");
  const body = $("start-body");
  body.replaceChildren();
  const nothing = preview.apps.length === 0 && preview.missing.length === 0 && preview.pinned === 0;

  body.append(
    el(
      "p",
      "",
      preview.profile_name
        ? `Using the profile "${preview.profile_name}".`
        : "No profile is selected, so only pinned windows are included.",
    ),
  );
  if (preview.apps.length || preview.pinned) {
    body.append(el("h3", "", "Will be protected now"));
    const items = el("ul");
    for (const app of preview.apps) {
      const where = app.path_restricted ? ", from its saved location only" : "";
      items.append(el("li", "", `${app.exe}: ${plural(app.windows, "open window")}${where}`));
    }
    if (preview.pinned) items.append(el("li", "", plural(preview.pinned, "pinned window")));
    body.append(items);
  }
  if (preview.missing.length) {
    body.append(el("h3", "", "Not running, so not protected yet"));
    const items = el("ul");
    for (const exe of preview.missing) items.append(el("li", "", `${exe}: protected if you open it during the session`));
    body.append(items);
  }
  if (nothing) {
    body.append(el("p", "error", "There is nothing to protect yet. Pick a profile with apps in it, or pin a window."));
  }
  $("start-interval").textContent = preview.watch_interval_ms;
  $("start-confirm").disabled = nothing;
  $("start-dialog").showModal();
}

$("start-dialog").addEventListener("close", (event) => {
  if (event.target.returnValue === "start") run(() => invoke("start_session"));
});

$("session-button").addEventListener("click", () => {
  if (model?.summary.session_active) run(() => invoke("end_session"));
  else openStart();
});

// -------------------------------------------------------------------- profiles

let editing = null; // the profile being edited, or null for a new one

function openProfile(profile) {
  editing = profile;
  $("profile-title").textContent = profile ? "Edit profile" : "New profile";
  $("profile-name").value = profile?.name ?? "";
  $("profile-error").textContent = "";
  $("profile-delete").hidden = !profile;
  $("profile-duplicate").hidden = !profile;

  // Offer every program with an open window, plus saved ones that are not running.
  const apps = new Map(); // exe -> { exe, name, path, running }
  for (const w of model.windows) {
    if (w.own || !w.exe) continue;
    if (!apps.has(w.exe)) apps.set(w.exe, { exe: w.exe, name: w.process, path: w.path, running: true });
  }
  const saved = new Map((profile?.rules ?? []).map((rule) => [rule.exe, rule]));
  for (const rule of saved.values()) {
    if (!apps.has(rule.exe)) apps.set(rule.exe, { exe: rule.exe, name: rule.exe, path: rule.path ?? "", running: false });
  }

  const container = $("profile-apps");
  container.replaceChildren();
  if (apps.size === 0) container.append(el("p", "note", "Open the apps you want to protect, then come back here."));
  for (const app of [...apps.values()].sort((a, b) => a.exe.localeCompare(b.exe))) {
    const rule = saved.get(app.exe);
    const item = el("div", "app-choice");
    item.dataset.exe = app.exe;
    item.dataset.path = rule?.path ?? app.path;

    const include = el("label", "check");
    const includeBox = el("input");
    includeBox.type = "checkbox";
    includeBox.className = "include";
    includeBox.checked = Boolean(rule);
    include.append(includeBox, el("span", "", app.exe), el("span", "muted", app.running ? "" : "not running"));

    const exact = el("label", "check sub");
    const exactBox = el("input");
    exactBox.type = "checkbox";
    exactBox.className = "exact";
    exactBox.checked = Boolean(rule?.path);
    exactBox.disabled = !item.dataset.path;
    exact.append(exactBox, el("span", "", "Only when run from this exact location"));
    const location = el("div", "path", item.dataset.path || "Location unknown");

    const sync = () => {
      exact.hidden = !includeBox.checked;
      location.hidden = !includeBox.checked || !exactBox.checked;
    };
    includeBox.addEventListener("change", sync);
    exactBox.addEventListener("change", sync);
    sync();

    item.append(include, exact, location);
    container.append(item);
  }
  $("profile-dialog").showModal();
}

function collectRules() {
  return [...document.querySelectorAll("#profile-apps .app-choice")]
    .filter((item) => item.querySelector(".include").checked)
    .map((item) => ({
      exe: item.dataset.exe,
      path: item.querySelector(".exact").checked ? item.dataset.path : null,
    }));
}

/** Try a profile change; keep the dialog open with the reason if it is refused. */
async function profileAction(action) {
  try {
    await action();
    $("profile-dialog").close("done");
  } catch (error) {
    $("profile-error").textContent = String(error);
  }
  await refresh();
}

$("profile-form").addEventListener("submit", (event) => {
  if (event.submitter?.value !== "save") return;
  event.preventDefault();
  profileAction(() =>
    invoke("save_profile", { id: editing?.id ?? null, name: $("profile-name").value, rules: collectRules() }),
  );
});
$("profile-delete").addEventListener("click", () => profileAction(() => invoke("delete_profile", { id: editing.id })));
$("profile-duplicate").addEventListener("click", () =>
  profileAction(() => invoke("duplicate_profile", { id: editing.id })),
);
$("new-profile").addEventListener("click", () => openProfile(null));

// ------------------------------------------------------------------- shortcuts

let capturing = null; // the action whose new combination is being recorded

function renderShortcutRows() {
  const message = $("shortcut-message");
  $("shortcut-rows").replaceChildren(
    ...model.shortcuts.map((shortcut) => {
      const row = el("div", "shortcut-row");
      const text = el("div");
      text.append(el("div", "", shortcut.label));
      const current = el("div", "muted");
      current.textContent =
        capturing === shortcut.action
          ? "Press the new combination, or Esc to cancel…"
          : `${showKeys(shortcut.keys)}${shortcut.registered ? "" : " (not working: already in use)"}`;
      text.append(current);
      const change = el("button", "", capturing === shortcut.action ? "Cancel" : "Change");
      change.type = "button";
      change.addEventListener("click", () => {
        capturing = capturing === shortcut.action ? null : shortcut.action;
        message.textContent = "";
        renderShortcutRows();
      });
      row.append(text, change);
      return row;
    }),
  );
}

/** Turn a key press into the text form the backend understands, e.g. "Control+Alt+H". */
function describeKeys(event) {
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return null;
  const parts = [];
  if (event.ctrlKey) parts.push("Control");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");
  parts.push(event.code.replace(/^Key/, "").replace(/^Digit/, ""));
  return parts.join("+");
}

$("shortcut-dialog").addEventListener("keydown", async (event) => {
  if (!capturing) return;
  event.preventDefault();
  event.stopPropagation();
  if (event.key === "Escape") {
    capturing = null;
    renderShortcutRows();
    return;
  }
  const keys = describeKeys(event);
  if (!keys) return;
  const action = capturing;
  capturing = null;
  const message = $("shortcut-message");
  try {
    await invoke("set_shortcut", { action, keys });
    message.textContent = `Saved: ${showKeys(keys)}.`;
  } catch (error) {
    message.textContent = String(error);
  }
  await refresh();
  renderShortcutRows();
});

$("shortcut-reset").addEventListener("click", async () => {
  const message = $("shortcut-message");
  capturing = null;
  try {
    await invoke("reset_shortcuts");
    message.textContent = "Shortcuts are back to their defaults.";
  } catch (error) {
    message.textContent = String(error);
  }
  await refresh();
  renderShortcutRows();
});

$("open-shortcuts").addEventListener("click", () => {
  capturing = null;
  $("shortcut-message").textContent = "";
  renderShortcutRows();
  $("shortcut-dialog").showModal();
});

// ----------------------------------------------------------------- diagnostics

$("open-diagnostics").addEventListener("click", async () => {
  $("diagnostics-message").textContent = "";
  $("diagnostics-text").textContent = await invoke("diagnostics");
  $("diagnostics-dialog").showModal();
});

$("diagnostics-save").addEventListener("click", async () => {
  const message = $("diagnostics-message");
  try {
    message.textContent = `Saved to ${await invoke("save_diagnostics")}`;
  } catch (error) {
    message.textContent = String(error);
  }
});

// ------------------------------------------------------------------------ misc

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
$("dismiss-notices").addEventListener("click", () => run(() => invoke("dismiss_notices")));
$("restart-elevated").addEventListener("click", () => run(() => invoke("restart_elevated")));

// Title bar buttons. Closing sends the window to the tray (handled in Rust).
const appWindow = window.__TAURI__.window?.getCurrentWindow();
$("win-min").addEventListener("click", () => appWindow?.minimize());
$("win-max").addEventListener("click", () => appWindow?.toggleMaximize());
$("win-close").addEventListener("click", () => appWindow?.close());

// A maximised window fills the screen, so it drops the rounded corners and shadow margin.
async function syncMaximized() {
  document.body.classList.toggle("maximized", Boolean(await appWindow?.isMaximized()));
}
appWindow?.onResized(syncMaximized);
syncMaximized();

document.addEventListener("contextmenu", (event) => event.preventDefault());
listen("state-changed", refresh);
window.addEventListener("focus", refresh);
setInterval(refresh, 2000);
refresh();
