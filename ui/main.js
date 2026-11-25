// Everything lives inside one function, so features.js (loaded after this file) can pick
// its own names freely. What it may use is published as window.Hidely further down.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  const $ = (id) => document.getElementById(id);
  const list = $("windows");
  const search = $("search");
  const errorLine = $("status");
  const rows = new Map(); // hwnd -> row parts
  let model = null; // the last snapshot from the backend, the single source of truth
  let filter = "all";
  let selected = null; // hwnd of the row the keyboard acts on
  let lastIndex = 0; // where the selection was, so a neighbour takes over when its row goes
  let failedRow = null; // hwnd of the row whose last change was refused
  const renderHooks = []; // features.js callbacks, run after every render

  const TRIANGLE = '<path d="M8 2.200 14.200 13H1.800L8 2.200Z"/><path d="M8 6.500v3M8 11.300v.100"/>';

  /** How each status is shown: a distinct shape and words, never colour alone. The badge says
      what the user gets; "Protection applied" stays in the tooltip. */
  const STATUS = {
    unprotected: { label: "Unprotected", icon: '<circle cx="8" cy="8" r="5.500"/>' },
    applying: {
      label: "Applying…",
      icon: '<path d="M8 2.500a5.500 5.500 0 1 0 5.500 5.500"/>',
    },
    applied: {
      label: "Hidden from capture",
      icon: '<circle cx="8" cy="8" r="5.500"/><path d="m5.600 8.200 1.700 1.700 3.200-3.500"/>',
    },
    unsupported: { label: "Visible to viewers", icon: TRIANGLE },
    error: { label: "Visible to viewers", icon: TRIANGLE },
  };

  const FILTERS = {
    all: {
      title: "All windows",
      match: () => true,
      empty: () => ["No other windows are open.", "Open an app and it appears here."],
    },
    pinned: {
      title: "Pinned",
      match: (w) => w.pinned,
      empty: () => ["Nothing is pinned.", "Pin a window to protect it every time a session starts."],
    },
    protected: {
      title: "Protected",
      match: (w) => w.status === "applied",
      empty: () =>
        model?.summary?.session_active
          ? ["The session is on, but no window is protected yet.", ""]
          : ["Nothing is protected.", "Choose Protect on a window, or start a session."],
    },
    problems: {
      title: "Needs attention",
      match: (w) => w.status === "error" || w.status === "unsupported",
      // Not "everything is protected": that would be untrue when nothing was chosen.
      empty: () => ["Nothing needs attention.", ""],
    },
  };

  /** The sidebar has room for a word or two; the full label is the tooltip. */
  const SHORT_LABEL = { toggle: "Session", restore: "Restore all" };

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

  /** Write text only when it differs: the list is re-read every two seconds, and an
      unchanged row should cost nothing. */
  const setText = (node, text) => {
    if (node.textContent !== text) node.textContent = text;
  };
  const setAttr = (node, name, value) => {
    if (node.getAttribute(name) !== value) node.setAttribute(name, value);
  };

  /** "42 min", "1 h 5 min". Minutes only, so it changes once a minute at most. */
  function duration(seconds, short = "under a minute") {
    const minutes = Math.floor(Math.max(0, seconds) / 60);
    if (minutes < 1) return short;
    if (minutes < 60) return `${minutes} min`;
    const rest = minutes % 60;
    return rest ? `${Math.floor(minutes / 60)} h ${rest} min` : `${Math.floor(minutes / 60)} h`;
  }

  /** How long ago Windows last confirmed a window, in coarse steps. It is worked out when
      the list is re-read, never by a timer of its own. Null if the backend did not say. */
  function checkedAge(verifiedUnix, now) {
    if (!verifiedUnix) return null;
    const age = Math.max(0, now - verifiedUnix);
    if (age < 10) return "just now";
    if (age < 60) return `${Math.floor(age / 10) * 10}s ago`;
    return `${Math.floor(age / 60)} min ago`;
  }

  function showError(text) {
    errorLine.textContent = text;
  }

  /** A new action starts with a clean slate: the last failure has been seen. */
  function clearError() {
    errorLine.textContent = "";
    rows.get(failedRow)?.tr.classList.remove("failed");
    failedRow = null;
  }

  function shake(node) {
    const done = () => {
      node.classList.remove("shake");
      node.removeEventListener("animationend", done);
      clearTimeout(timer);
    };
    node.classList.add("shake");
    node.addEventListener("animationend", done);
    // With reduced motion no animation runs, so time clears it (and the listener) instead.
    const timer = setTimeout(done, 300);
  }

  /** Run an action, show its error (if any), then re-read the real state. */
  async function run(action) {
    clearError();
    try {
      await action();
    } catch (error) {
      showError(String(error));
    }
    dropOlderSnapshots();
    await refresh();
  }

  // ------------------------------------------------------------------ window list

  function createRow(hwnd) {
    const tr = el("tr");
    tr.id = `w${hwnd}`;
    tr.hwndKey = hwnd;
    tr.tabIndex = -1; // one row at a time is the list's tab stop (see select)
    tr.addEventListener("click", () => select(hwnd));

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

    // The buttons stay out of the Tab order: the row is the stop, and Enter and P drive it.
    const actionsCell = el("td", "actions-cell");
    const actions = el("div", "actions");
    const pin = el("button", "pin");
    pin.type = "button";
    pin.tabIndex = -1;
    pin.innerHTML = PIN_ICON;
    pin.addEventListener("click", () => togglePin(hwnd));
    const toggle = el("button", "toggle");
    toggle.type = "button";
    toggle.tabIndex = -1;
    toggle.addEventListener("click", () => toggleRow(hwnd));
    actions.append(pin, toggle);
    actionsCell.append(actions);

    tr.append(nameCell, app, statusCell, actionsCell);
    return { tr, title, app, badge, badgeIcon, badgeLabel, detail, pin, toggle, data: null, icon: null };
  }

  /** The words for one window's status: badge, the line under it, and the tooltip. */
  function describe(data, now) {
    const status = STATUS[data.status] ?? STATUS.unprotected;
    const view = { icon: status.icon, label: status.label, detail: "", tip: data.detail ?? "" };
    if (data.status === "applied" && data.owned) {
      const age = checkedAge(data.verified_unix, now);
      if (age) {
        view.detail = `Checked ${age}`;
        view.tip = `Protection applied. Windows confirmed this, checked ${age}.`;
      } else {
        view.tip = `Protection applied. ${data.detail ?? ""}`.trim();
      }
    } else if (data.status === "applied") {
      view.detail = data.detail ?? ""; // protected by the app itself, not by Hidely
    } else if (data.status === "unsupported") {
      view.detail = data.detail ?? "";
    } else if (data.status === "error" && data.owned) {
      // Undoing failed: the window is still left out of captures.
      view.label = "Still hidden";
      view.detail = data.detail ?? "";
    } else if (data.status === "error") {
      view.detail = `${data.detail ?? "Not hidden."} Retry, or check Diagnostics.`;
    }
    return view;
  }

  function updateRow(row, data, now) {
    const before = row.data?.status; // undefined the first time a row is drawn
    row.data = data;
    const view = describe(data, now);
    setAttr(row.tr, "data-status", data.status);
    // While a change is pending the row is announced with the pending words, like the badge.
    setAttr(row.tr, "aria-label", `${data.title}, ${row.tr.dataset.pending ? row.badgeLabel.textContent : view.label}`);
    setText(row.title, data.title);
    setAttr(row.title, "title", data.title);
    setText(row.app, data.process);

    if (row.icon !== view.icon) {
      row.icon = view.icon;
      row.badgeIcon.innerHTML = view.icon;
    }
    setAttr(row.badge, "title", view.tip);
    setText(row.detail, view.detail);
    row.detail.hidden = !view.detail;

    setAttr(row.pin, "aria-pressed", String(data.pinned));
    setAttr(row.pin, "aria-label", data.pinned ? `Unpin ${data.title}` : `Pin ${data.title}`);
    setAttr(
      row.pin,
      "title",
      data.pinned ? "Pinned: protected when a session starts (P)" : "Pin, to protect this window when a session starts (P)",
    );

    const busy = data.status === "applying";
    const byApp = data.status === "applied" && !data.owned;
    const kind = byApp ? "byapp" : data.owned ? "unprotect" : data.status === "error" ? "retry" : "protect";
    setAttr(row.toggle, "data-kind", kind);
    row.toggle.disabled = busy || byApp;
    setAttr(
      row.toggle,
      "title",
      byApp
        ? "This app protects this window itself, so there is nothing for Hidely to do"
        : data.owned
          ? "Make this window capturable again"
          : "Exclude this window from screen capture",
    );

    // While a click is on its way to the backend the row shows the optimistic words
    // (see toggleRow); the real ones return with the refresh that follows.
    if (!row.tr.dataset.pending) {
      setText(row.badgeLabel, view.label);
      setText(row.toggle, { byapp: "By app", unprotect: "Unprotect", retry: "Retry", protect: "Protect" }[kind]);
    }

    if (before && before !== data.status) {
      // A new status can change the row's height; keep the row the keyboard is on in view.
      if (row.tr === document.activeElement) row.tr.scrollIntoView({ block: "nearest" });
      if (data.status === "applied" && data.owned) {
        // Windows has just confirmed it: one green bar on the left edge, then gone.
        row.tr.classList.add("settle");
        setTimeout(() => row.tr.classList.remove("settle"), 600);
      } else if (data.status === "error") {
        shake(row.badge);
      }
    }
  }

  /** Protect or release one window. The row answers at once, but it only ever says
      "Applying…" or "Removing…": "Hidden from capture" waits for the backend's read-back. */
  async function toggleRow(hwnd) {
    const row = rows.get(hwnd);
    if (!row || row.toggle.disabled || row.tr.dataset.pending) return;
    const want = !row.data.owned;
    clearError();
    row.tr.dataset.pending = "1";
    row.badgeLabel.textContent = want ? "Applying…" : "Removing…";
    row.tr.setAttribute("aria-label", `${row.data.title}, ${row.badgeLabel.textContent}`);
    row.toggle.textContent = want ? "Unprotect" : "Protect";
    try {
      await invoke("set_protected", { hwnd, protected: want });
    } catch (error) {
      // Loud, and it stays: the message and the red bar remain until the next action.
      failedRow = hwnd;
      row.tr.classList.add("failed");
      shake(row.toggle);
      showError(`Couldn't ${want ? "protect" : "release"} "${row.data.title}": ${error}`);
    } finally {
      delete row.tr.dataset.pending;
      dropOlderSnapshots();
      await refresh();
      // The message takes room under the list; the row it is about must stay in sight.
      if (failedRow === hwnd) row.tr.scrollIntoView({ block: "nearest" });
    }
  }

  function togglePin(hwnd) {
    const row = rows.get(hwnd);
    if (!row) return Promise.resolve();
    return run(() => invoke("set_pinned", { hwnd, pinned: !row.data.pinned }));
  }

  // -------------------------------------------------------------------- keyboard

  const visibleRows = () => [...list.children].filter((tr) => !tr.hidden);

  /** Make one row the selected one. Rows are never recreated, so the selection and the
      keyboard focus both survive the periodic re-read. */
  function select(hwnd, focus = true) {
    const old = rows.get(selected)?.tr;
    if (old && selected !== hwnd) {
      old.classList.remove("sel");
      old.tabIndex = -1;
    }
    const tr = rows.get(hwnd)?.tr;
    if (!tr) {
      selected = null;
      return;
    }
    selected = hwnd;
    tr.classList.add("sel");
    tr.tabIndex = 0;
    lastIndex = Math.max(0, visibleRows().indexOf(tr));
    if (focus) {
      tr.focus({ preventScroll: true });
      tr.scrollIntoView({ block: "nearest" }); // no smooth scrolling: the keyboard must feel instant
    }
  }

  /** After rows went away or were filtered out: if the selected one is gone, its neighbour
      takes over. Focus follows only if it was in the list already. */
  function keepSelection(hadFocus) {
    const visible = visibleRows();
    const current = rows.get(selected)?.tr;
    if (current && !current.hidden) {
      lastIndex = visible.indexOf(current);
      return;
    }
    const next = visible[Math.min(lastIndex, visible.length - 1)];
    if (next) {
      select(next.hwndKey, hadFocus && !document.querySelector("dialog[open]"));
    } else {
      select(null);
    }
  }

  function focusList() {
    keepSelection(false);
    if (selected) select(selected);
  }

  list.addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.altKey || event.metaKey || document.querySelector("dialog[open]")) return;
    const visible = visibleRows();
    const index = visible.indexOf(rows.get(selected)?.tr);
    const go = (to) => {
      const tr = visible[Math.max(0, Math.min(visible.length - 1, to))];
      if (tr) select(tr.hwndKey);
    };
    switch (event.key) {
      case "ArrowDown":
        go(index + 1);
        break;
      case "ArrowUp":
        go(index - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(visible.length - 1);
        break;
      case "Enter":
      case " ":
        if (event.repeat) break;
        if (selected) toggleRow(selected);
        break;
      case "p":
      case "P":
        if (event.repeat) break;
        if (selected) togglePin(selected);
        break;
      default:
        return;
    }
    event.preventDefault();
  });

  // "/" jumps to the search field from anywhere outside a text field or a dialog.
  document.addEventListener("keydown", (event) => {
    if (event.key !== "/" || event.ctrlKey || event.altKey || event.metaKey) return;
    if (event.target.matches?.("input, textarea, [contenteditable]")) return;
    if (document.querySelector("dialog[open]")) return;
    event.preventDefault();
    search.focus();
    search.select();
  });

  search.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "Enter") {
      event.preventDefault();
      focusList();
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (search.value) {
        search.value = "";
        applyFilter();
      } else {
        focusList();
      }
    }
  });

  function applyFilter(hadFocus = list.contains(document.activeElement)) {
    const query = search.value.trim().toLowerCase();
    const { match, title, empty } = FILTERS[filter];
    let shown = 0;
    for (const row of rows.values()) {
      const text = `${row.data.title} ${row.data.process}`.toLowerCase();
      const visible = match(row.data) && (!query || text.includes(query));
      if (row.tr.hidden === visible) row.tr.hidden = !visible;
      if (visible) shown++;
    }
    setText($("list-title"), title);
    setText($("total"), plural(shown, "window"));

    const [headline, hint] = query
      ? [`No windows match "${search.value.trim()}".`, "Search covers window names and app names."]
      : empty();
    const box = $("empty");
    box.hidden = shown > 0;
    if (box.dataset.text !== headline + hint) {
      box.dataset.text = headline + hint;
      box.replaceChildren(el("strong", "", headline), ` ${hint}`);
    }
    keepSelection(hadFocus);
  }

  // --------------------------------------------------------------------- render

  function render(snapshot) {
    model = snapshot;
    const { windows, summary } = snapshot;
    const now = Date.now() / 1000;
    const hadFocus = list.contains(document.activeElement); // before any row is removed

    const seen = new Set();
    for (const data of windows) {
      seen.add(data.hwnd);
      let row = rows.get(data.hwnd);
      if (!row) {
        row = createRow(data.hwnd);
        rows.set(data.hwnd, row);
        list.append(row.tr);
      }
      updateRow(row, data, now);
    }
    for (const [hwnd, row] of rows) {
      if (!seen.has(hwnd)) {
        row.tr.remove();
        rows.delete(hwnd);
      }
    }

    const count = (test) => windows.filter(test).length;
    const problems = count(FILTERS.problems.match);
    setText($("count-all"), String(windows.length));
    setText($("count-pinned"), String(count(FILTERS.pinned.match)));
    setText($("count-protected"), String(count(FILTERS.protected.match)));
    setText($("count-problems"), String(problems));
    $("count-problems").classList.toggle("warn", problems > 0);
    applyFilter(hadFocus);

    renderSession(summary, snapshot.last_end, now);
    renderProfiles(snapshot);
    renderHotkeys(snapshot.shortcuts ?? []);
    renderNotices(snapshot.notices ?? []);

    // Offered only when it would help: Hidely is not elevated yet.
    const restart = $("restart-elevated");
    restart.hidden = snapshot.elevated;
    restart.title = "Needed to protect windows of apps that run as administrator. Ends the current session first.";

    for (const hook of renderHooks) {
      try {
        hook(snapshot);
      } catch (error) {
        console.error(error); // a broken extra must not stop the list from drawing
      }
    }
  }

  /** What the last session did, said once it is over: [title, detail]. "Restore all" with
      no session running also leaves a report, as does an older backend; neither has a
      start time, so those get the short form instead of "0 windows hidden". */
  function endSummary(end) {
    const failed = end.failed ?? 0;
    const unrestored =
      failed > 0
        ? `${failed} could not be restored and may still be hidden from capture. Closing those apps clears it.`
        : "";
    if (end.started_unix == null) {
      return [
        failed > 0 ? "Some windows could not be restored" : "Everything is capturable again",
        `${plural(end.restored ?? 0, "window")} capturable again. ${unrestored}`.trim(),
      ];
    }

    const length = duration(end.duration_secs ?? 0);
    const caught = end.caught_by_rules ?? 0;
    const lost = end.protection_lost ?? 0;
    let text = `${length[0].toUpperCase()}${length.slice(1)}. ${plural(end.windows_protected ?? end.restored ?? 0, "window")} hidden`;
    if (caught > 0) text += `, ${caught} caught as ${caught === 1 ? "it" : "they"} opened`;
    text += ".";
    if (lost > 0) {
      const again = end.reapplied ?? 0;
      text += ` ${lost} lost protection along the way${again > 0 ? `, ${again} put back` : ""}.`;
    }
    return ["Session ended", `${text} ${unrestored || "Everything is capturable again."}`];
  }

  function renderSession(summary, lastEnd, now) {
    const lost = summary.protection_lost ?? 0;
    const problems = summary.errors + summary.unsupported;
    const state = $("state");
    state.classList.toggle("active", summary.session_active);
    state.classList.toggle("attention", problems > 0);

    const button = $("session-button");
    if (!summary.session_active) {
      setText($("summary"), problems > 0 ? `No session · ${problems} need attention` : "No privacy session");
      const [title, detail] = lastEnd
        ? endSummary(lastEnd)
        : ["No privacy session", "Nothing is protected. Pick a profile or pin windows, then start."];
      setText($("session-title"), title);
      setText($("session-detail"), detail);
      setText(button, "Start session");
      button.classList.add("primary");
      return;
    }

    // An older backend may not say when the session started; then the sentence is left out.
    const started = summary.started_unix
      ? `Started ${new Date(summary.started_unix * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}. `
      : "";
    const parts = [`${summary.applied} protected`];
    if (summary.applying) parts.push(`${summary.applying} applying`);
    if (summary.unsupported) parts.push(`${summary.unsupported} unsupported`);
    if (summary.errors) parts.push(plural(summary.errors, "error"));
    // The length only moves once a minute, so the header is rewritten once a minute too.
    const length = summary.started_unix ? [duration(now - summary.started_unix, "under 1 min")] : [];
    setText($("summary"), ["Session on", ...length, ...parts].join(" · "));
    setText(
      $("session-title"),
      summary.profile_name ? `Privacy session on · ${summary.profile_name}` : "Privacy session on",
    );
    const partial = problems > 0 && summary.applied > 0 ? " Only some of your windows are protected." : "";
    const slipped = lost > 0 ? ` ${plural(lost, "window")} lost protection during this session.` : "";
    const watching = summary.watching
      ? " New windows of this profile's apps are protected as they open."
      : " Only the windows you choose are protected.";
    setText($("session-detail"), `${started}${parts.join(", ")}.${partial}${slipped}${watching}`);
    setText(button, "End session");
    button.classList.remove("primary");
  }

  // The sidebar is rebuilt only when what it shows has changed. Rebuilding it on every
  // re-read would throw away keyboard focus there every two seconds.
  let profilesShown = "";
  let hotkeysShown = "";
  let noticesShown = "";

  function renderProfiles({ profiles, selected_profile: chosen }) {
    const key = JSON.stringify([profiles, chosen]);
    if (key === profilesShown) return;
    profilesShown = key;

    const hint = $("profiles-hint");
    hint.textContent = profiles.length
      ? "Choosing a profile doesn't start anything."
      : "A profile is a list of apps to protect each session. Create one with New.";
    hint.classList.toggle("always", profiles.length === 0);

    const container = $("profiles");
    // Rebuilding drops keyboard focus; remember which profile (and which of its two
    // buttons) had it, to hand it back below.
    const focused = container.contains(document.activeElement) ? document.activeElement : null;
    const focusedId = focused?.closest(".profile")?.dataset.profile;
    const focusedEdit = Boolean(focused?.classList.contains("link"));
    const options = [{ id: null, name: "No profile" }, ...profiles];
    container.replaceChildren(
      ...options.map((profile) => {
        const row = el("div", "profile");
        row.dataset.profile = profile.id ?? "";
        const choose = el("button", "profile-choose");
        choose.type = "button";
        choose.setAttribute("role", "radio");
        choose.setAttribute("aria-checked", String(profile.id === chosen));
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
    if (focused) {
      const row = [...container.children].find((item) => item.dataset.profile === focusedId) ?? container.firstElementChild;
      ((focusedEdit && row?.querySelector(".link")) || row?.querySelector(".profile-choose"))?.focus();
    }
  }

  // Arrow keys move between the profile radios and choose the one they land on, as a
  // radio group does.
  $("profiles").addEventListener("keydown", (event) => {
    const step = { ArrowDown: 1, ArrowRight: 1, ArrowUp: -1, ArrowLeft: -1 }[event.key];
    if (!step || event.ctrlKey || event.altKey || event.metaKey || !event.target.matches(".profile-choose")) return;
    event.preventDefault();
    const radios = [...$("profiles").querySelectorAll(".profile-choose")];
    const next = radios[(radios.indexOf(event.target) + step + radios.length) % radios.length];
    next.focus();
    if (next.getAttribute("aria-checked") !== "true") next.click();
  });

  function renderHotkeys(shortcuts) {
    const key = JSON.stringify(shortcuts);
    if (key === hotkeysShown) return;
    hotkeysShown = key;
    $("hotkeys").replaceChildren(
      ...shortcuts.map((shortcut) => {
        const item = el("div");
        const keys = el("dt");
        for (const part of showKeys(shortcut.keys).split("+")) keys.append(el("kbd", "", part));
        const short = SHORT_LABEL[shortcut.action] ?? shortcut.label;
        item.classList.toggle("failed", !shortcut.registered);
        item.title = shortcut.registered
          ? shortcut.label
          : `${shortcut.label} (not working: another app uses this shortcut)`;
        item.append(keys, el("dd", "", short));
        // A shortcut that failed to register says so on a line of its own.
        if (!shortcut.registered) item.append(el("dd", "why", "Not working: another app uses it"));
        return item;
      }),
    );
  }

  function renderNotices(notices) {
    const key = JSON.stringify(notices);
    if (key === noticesShown) return;
    noticesShown = key;
    $("notices").hidden = notices.length === 0;
    $("notice-text").replaceChildren(...notices.map((notice) => el("p", "", notice)));
  }

  // Refreshes are asked for from many places (the timer, events, focus, every action) and
  // their answers can arrive out of order. Each request is numbered, and an answer older
  // than what is already on screen is dropped, so a row never flips back.
  let asked = 0; // number of the newest snapshot request
  let oldestWanted = 0; // answers to requests below this are out of date

  /** After an action: snapshots requested before it finished no longer describe the state. */
  function dropOlderSnapshots() {
    oldestWanted = asked + 1;
  }

  /** Re-read the state and draw it. Never rejects: a failed read is logged and the next
      one tries again. */
  async function refresh() {
    const number = ++asked;
    try {
      const snapshot = await invoke("snapshot");
      if (number < oldestWanted) return;
      oldestWanted = number;
      render(snapshot);
    } catch (error) {
      console.error("Hidely: could not refresh", error);
    }
  }

  /** What features.js (the palette, "What viewers see", the tour, the meeting offer) may use. */
  window.Hidely = {
    invoke,
    listen,
    run,
    refresh,
    get model() {
      return model;
    },
    get selected() {
      return selected;
    },
    rowData: (hwnd) => rows.get(hwnd)?.data,
    toggleRow,
    togglePin,
    focusList,
    onRender(fn) {
      renderHooks.push(fn);
    },
    showKeys,
  };

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

  // "test yours" opens the test page in the default browser, never inside this window.
  $("start-test-page").addEventListener("click", async () => {
    try {
      await invoke("open_test_page");
    } catch (error) {
      $("start-dialog").close("cancel");
      showError(String(error));
    }
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

  // -------------------------------------------------------------------- settings

  const LATEST_RELEASE = "https://api.github.com/repos/CodingRule/Hidely/releases/latest";

  /** Ask GitHub for the newest release. Returns its version if newer than this one, else null. */
  async function findUpdate() {
    const response = await fetch(LATEST_RELEASE, { headers: { Accept: "application/vnd.github+json" } });
    if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
    const latest = (await response.json()).tag_name ?? "";
    return (await invoke("is_newer", { latest })) ? latest.replace(/^v/, "") : null;
  }

  function offerUpdate(version) {
    const link = $("update-available");
    link.textContent = `Version ${version} is available`;
    link.hidden = false;
  }

  $("update-available").addEventListener("click", () => run(() => invoke("open_releases")));

  $("open-settings").addEventListener("click", () => {
    $("setting-startup").checked = model.starts_with_windows;
    $("setting-updates").checked = model.check_updates_at_startup;
    $("setting-browsers").checked = model.browsers_connected;
    $("settings-version").textContent = `Hidely ${model.version}`;
    $("settings-message").textContent = "";
    $("settings-dialog").showModal();
  });

  /** Apply a checkbox; if the change is refused, say why and put the box back. */
  async function applySetting(box, command) {
    const message = $("settings-message");
    message.textContent = "";
    try {
      await invoke(command, { enabled: box.checked });
    } catch (error) {
      message.textContent = String(error);
    }
    await refresh();
    $("setting-startup").checked = model.starts_with_windows;
    $("setting-updates").checked = model.check_updates_at_startup;
    $("setting-browsers").checked = model.browsers_connected;
  }

  $("setting-startup").addEventListener("change", (event) => applySetting(event.target, "set_start_with_windows"));
  $("setting-updates").addEventListener("change", (event) => applySetting(event.target, "set_check_updates"));
  $("setting-browsers").addEventListener("change", (event) => applySetting(event.target, "set_browsers_connected"));

  $("check-updates").addEventListener("click", async () => {
    const message = $("settings-message");
    message.textContent = "Checking…";
    try {
      const version = await findUpdate();
      if (version) {
        offerUpdate(version);
        message.textContent = `Version ${version} is available. Use the link at the bottom left of the window to get it.`;
      } else {
        message.textContent = "You have the latest version.";
      }
    } catch (error) {
      message.textContent = `Could not check: ${error.message}. Are you online?`;
    }
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
  search.addEventListener("input", () => applyFilter());
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
  // Nothing is redrawn while the window is hidden (in the tray or minimised); it catches up
  // the moment it shows again.
  setInterval(() => {
    if (!document.hidden) refresh();
  }, 2000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) refresh();
  });
  refresh().then(async () => {
    // Only if the user asked for it: one quiet check when the app starts.
    if (!model?.check_updates_at_startup) return;
    try {
      const version = await findUpdate();
      if (version) offerUpdate(version);
    } catch {
      // Offline or GitHub unavailable: say nothing, the user can check from Settings.
    }
  });
})();
