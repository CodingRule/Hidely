// Hidely v0.4 features: Ctrl+K palette, "What viewers see", first-run tour, meeting-app offer
// and the lost-protection notice. Builds its own DOM and talks to main.js only through
// window.Hidely, so a missing hook switches a feature off instead of breaking the window.
(() => {
  const H = window.Hidely;
  if (!H) return;

  const $ = (id) => document.getElementById(id);
  const el = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const button = (text, className) => {
    const node = el("button", className, text);
    node.type = "button";
    return node;
  };
  const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;
  const showKeys = (keys) => {
    try {
      return H.showKeys ? H.showKeys(keys) : String(keys).replaceAll("Super", "Win");
    } catch {
      return String(keys);
    }
  };
  /** Keys as <kbd> parts, e.g. "Win+Shift+H" -> three keys. */
  const kbds = (keys) => {
    const span = el("span", "keys");
    for (const key of String(keys).split("+")) if (key) span.append(el("kbd", "", key));
    return span;
  };
  const model = () => H.model ?? null;
  const windows = () => (Array.isArray(model()?.windows) ? model().windows : []);
  const session = () => Boolean(model()?.summary?.session_active);
  const shortcutKeys = (action) => {
    const found = (model()?.shortcuts ?? []).find((s) => s.action === action);
    return found?.keys ? showKeys(found.keys) : "";
  };
  const anyModalOpen = (except) =>
    [...document.querySelectorAll("dialog[open]")].some((dialog) => dialog !== except);
  const run = (action) =>
    typeof H.run === "function"
      ? H.run(action)
      : Promise.resolve()
          .then(action)
          .catch((error) => console.error(error));
  const invoke = (command, args) => H.invoke(command, args);
  const focusList = () => {
    try {
      H.focusList?.();
    } catch {
      // No list to focus: leave focus where the browser put it.
    }
  };
  const safe = (name, fn) => {
    try {
      fn();
    } catch (error) {
      console.error(`Hidely ${name}:`, error);
    }
  };
  const listen = (event, handler) => {
    try {
      Promise.resolve(H.listen?.(event, handler)).catch(() => {});
    } catch {
      // Older backend or no event API: the feature just never fires.
    }
  };

  // Renders queue here; each feature reads the same snapshot.
  const renderers = [];
  const onSnapshot = (snapshot) => {
    if (!snapshot) return;
    for (const fn of renderers) safe("render", () => fn(snapshot));
  };

  const slot = $("banner-slot");

  // ======================================================== lost-protection notice

  let lost = null; // { box, list }
  const lostItems = new Map(); // hwnd -> message, newest last

  function buildLost() {
    if (!slot) return;
    const box = el("div", "banner banner-lost");
    box.hidden = true;
    const body = el("div", "banner-body");
    const text = el("div");
    text.setAttribute("role", "alert");
    body.append(text);
    const dismiss = button("Dismiss", "link");
    dismiss.addEventListener("click", () => {
      lostItems.clear();
      box.hidden = true;
      // The button disappears with the notice, so give focus a sensible home.
      focusList();
    });
    box.append(body, dismiss);
    slot.prepend(box);
    lost = { box, text };
  }

  function showLost({ hwnd, app, reapplied } = {}) {
    if (!lost) return;
    const title = H.rowData?.(hwnd)?.title;
    const name = app || title || "A window";
    const message = reapplied
      ? `${name} lost protection for a moment. Hidely applied it again.`
      : `${name} lost protection and is visible to viewers again. Check Needs attention.`;
    lostItems.delete(String(hwnd));
    lostItems.set(String(hwnd), message);
    // Only the latest few; older ones are still in Diagnostics.
    while (lostItems.size > 4) lostItems.delete(lostItems.keys().next().value);
    lost.text.replaceChildren(...[...lostItems.values()].map((m) => el("p", "", m)));
    lost.box.hidden = false;
  }

  safe("lost notice", () => {
    buildLost();
    listen("protection-lost", (event) => showLost(event?.payload ?? {}));
  });

  // ============================================================ meeting-app offer

  let offerBox = null;
  let offerText = null;
  let offerShownKey = ""; // the offer currently on screen
  let offerDismissedKey = ""; // hidden locally after "Not now", until a new offer arrives
  let offerStarted = ""; // offer the user accepted, so it is cleared once the session starts

  function buildOffer() {
    if (!slot) return;
    offerBox = el("div", "banner banner-offer");
    offerBox.hidden = true;
    offerBox.setAttribute("role", "region");
    offerBox.setAttribute("aria-label", "Meeting app");
    offerText = el("p", "banner-text");
    offerText.setAttribute("aria-live", "polite");
    const body = el("div", "banner-body");
    body.append(offerText);
    const actions = el("div", "banner-actions");
    const start = button("Start session", "primary");
    start.addEventListener("click", () => {
      if (session()) return;
      offerStarted = offerShownKey;
      // The usual start dialog, so the user still sees what the session will affect.
      $("session-button")?.click();
    });
    const later = button("Not now", "link");
    later.addEventListener("click", () => {
      offerDismissedKey = offerShownKey;
      offerBox.hidden = true;
      focusList();
      run(() => invoke("dismiss_meeting_offer"));
    });
    actions.append(start, later);
    offerBox.append(body, actions);
    // After the lost notice, before the tour.
    if (lost?.box) lost.box.after(offerBox);
    else slot.prepend(offerBox);
  }

  function renderOffer(snapshot) {
    if (!offerBox) return;
    const offer = snapshot.meeting_offer;
    const key = offer ? `${offer.exe ?? ""}|${offer.process ?? ""}` : "";
    if (offerStarted && session()) {
      // Accepted: the offer has done its job; quietly clear it in the backend.
      if (key && key === offerStarted) invoke("dismiss_meeting_offer").catch(() => {});
      offerStarted = "";
    }
    const show = Boolean(offer) && !session() && snapshot.meeting_detection !== false && key !== offerDismissedKey;
    if (show && key !== offerShownKey) {
      offerText.textContent = `${offer.process || offer.exe || "A meeting app"} is open. Start a privacy session?`;
    }
    offerShownKey = show ? key : offerShownKey;
    if (!offer) offerDismissedKey = "";
    if (offerBox.hidden === show) {
      const hadFocus = offerBox.contains(document.activeElement);
      offerBox.hidden = !show;
      if (hadFocus && !show) focusList();
    }
  }

  safe("meeting offer", () => {
    buildOffer();
    renderers.push(renderOffer);
  });

  // The Settings switch for the offer.
  safe("meeting setting", () => {
    const box = $("setting-meeting");
    if (!box) return;
    box.addEventListener("change", async () => {
      const message = $("settings-message");
      if (message) message.textContent = "";
      try {
        await invoke("set_meeting_detection", { enabled: box.checked });
      } catch (error) {
        if (message) message.textContent = String(error);
      }
      // Re-read the real value; the render below puts the switch back if it was refused.
      await H.refresh?.();
    });
    renderers.push((snapshot) => {
      const known = typeof snapshot.meeting_detection === "boolean";
      box.disabled = !known; // an older backend has no such setting
      if (known) box.checked = snapshot.meeting_detection;
    });
  });

  // ================================================================ first-run tour

  const tour = {
    box: null,
    step: 1, // kept in memory only; worked out again from state after a restart
    ended: false, // set locally the moment the user finishes, before the backend confirms
    failed: false, // a step-1 attempt ended in error or unsupported
    lastKey: "",
    statuses: new Map(), // hwnd -> previous status, to see a fresh failure
    session: null, // previous session_active, to see a session start
  };

  function buildTour() {
    if (!slot) return;
    tour.box = el("div", "banner banner-tour");
    tour.box.hidden = true;
    tour.box.setAttribute("role", "region");
    tour.box.setAttribute("aria-label", "First-run tour");
    slot.append(tour.box);
  }

  function finishTour() {
    tour.ended = true;
    hideTour();
    // Not through run(): that would clear an error line the user may still need.
    invoke("set_first_run_done", { done: true })
      .then(() => H.refresh?.())
      .catch((error) => console.error(error));
  }

  function hideTour() {
    if (!tour.box || tour.box.hidden) return;
    const hadFocus = tour.box.contains(document.activeElement);
    tour.box.hidden = true;
    tour.lastKey = "";
    if (hadFocus) focusList();
  }

  function goToStep(step) {
    tour.step = step;
    tour.failed = false;
    onSnapshot(model());
  }

  function tourContent(step) {
    const toggle = shortcutKeys("toggle");
    const restore = shortcutKeys("restore");
    if (step === 1) {
      return {
        title: "Pick a window to hide.",
        text: "Choose Protect on any window below. It stays on your screen but leaves screen captures.",
        actions: [["Skip", "link", () => goToStep(2)]],
      };
    }
    if (step === 2) {
      return {
        title: "See it for yourself.",
        text: "Open the test page and share your screen. The window you protected should be missing.",
        actions: [
          [
            "Open test page",
            "primary",
            () => {
              goToStep(3);
              run(() => invoke("open_test_page"));
            },
          ],
          ["Skip", "link", () => goToStep(3)],
        ],
      };
    }
    const parts = [];
    parts.push(toggle ? ["Press ", kbds(toggle), " to start or end a privacy session from anywhere."] : [
      "Your session shortcut starts or ends a privacy session from anywhere.",
    ]);
    if (restore) parts.push([" ", kbds(restore), " makes everything capturable again."]);
    return {
      title: "One shortcut does it all.",
      text: parts.flat(),
      key: `${toggle}|${restore}`,
      actions: [["Done", "primary", finishTour]],
    };
  }

  function drawTour() {
    const { step, failed } = tour;
    const content = tourContent(step);
    const key = `${step}|${failed}|${content.key ?? ""}`;
    if (key === tour.lastKey && !tour.box.hidden) return;
    tour.lastKey = key;
    const hadFocus = tour.box.contains(document.activeElement);

    const body = el("div", "banner-body");
    const line = el("p", "banner-text");
    line.setAttribute("aria-live", "polite");
    line.append(el("strong", "", content.title), " ");
    for (const part of [].concat(content.text)) line.append(part);
    body.append(line);
    if (step === 1 && failed) {
      body.append(el("p", "banner-problem", "That one couldn't be protected. Try another window."));
    }

    const side = el("div", "banner-side");
    side.append(el("span", "banner-step", `Step ${step} of 3`));
    const actions = el("div", "banner-actions");
    for (const [label, className, handler] of content.actions) {
      const action = button(label, className);
      action.addEventListener("click", handler);
      actions.append(action);
    }
    if (step < 3) {
      const skip = button("Skip tour", "link");
      skip.addEventListener("click", finishTour);
      actions.append(skip);
    }
    side.append(actions);
    tour.box.replaceChildren(body, side);
    tour.box.hidden = false;
    // The old buttons are gone; keep keyboard users inside the banner, never pull others in.
    if (hadFocus) actions.querySelector("button")?.focus();
  }

  function renderTour(snapshot) {
    if (!tour.box) return;
    const active = Boolean(snapshot.summary?.session_active);
    const list = Array.isArray(snapshot.windows) ? snapshot.windows : [];
    const wasSession = tour.session;
    tour.session = active;

    if (snapshot.first_run_done !== false || tour.ended) {
      tour.statuses.clear();
      hideTour();
      return;
    }

    // On step 3 the toggle shortcut completes the tour, and it either starts or ends a
    // session. Earlier steps ignore this: protecting one window by hand also counts as a
    // session, and that is exactly what step 1 asks for.
    if (wasSession !== null && wasSession !== active && tour.step === 3) {
      finishTour();
      return;
    }

    if (tour.step === 1) {
      if (list.some((w) => w.owned && w.status === "applied")) {
        tour.step = 2;
        tour.failed = false;
      } else {
        for (const w of list) {
          const before = tour.statuses.get(w.hwnd);
          const bad = w.status === "error" || w.status === "unsupported";
          if (bad && before !== undefined && before !== w.status) tour.failed = true;
        }
      }
    }
    tour.statuses = new Map(list.map((w) => [w.hwnd, w.status]));
    drawTour();
  }

  function replayTour() {
    const message = $("settings-message");
    if (message) message.textContent = "";
    Promise.resolve()
      .then(() => invoke("set_first_run_done", { done: false }))
      .then(
        () => {
          tour.ended = false;
          tour.step = 1;
          tour.failed = false;
          tour.lastKey = "";
          tour.session = null;
          tour.statuses.clear();
          const settings = $("settings-dialog");
          if (settings?.open) settings.close();
          return H.refresh?.();
        },
        (error) => {
          if (message) message.textContent = String(error);
          else console.error(error);
        },
      );
  }

  safe("tour", () => {
    buildTour();
    renderers.push(renderTour);
    $("replay-tour")?.addEventListener("click", replayTour);
  });

  // ============================================================ What viewers see

  const viewer = {
    dialog: null,
    canvas: null,
    frame: null,
    message: null,
    caption: null,
    screen: null,
    pause: null,
    next: null,
    timer: 0,
    busy: false, // a frame is being fetched
    paused: false,
    monitor: undefined, // undefined lets the backend pick the screen with protected windows
    count: 1,
    hasFrame: false,
    generation: 0, // bumps on close, so a late frame is dropped
    again: false, // fetch again as soon as the current one lands (Next screen)
  };

  function buildViewer() {
    const dialog = el("dialog");
    dialog.id = "viewer-dialog";
    dialog.setAttribute("aria-labelledby", "viewer-title");
    const form = el("form");
    form.method = "dialog";

    const title = el("h2", "", "What viewers see");
    title.id = "viewer-title";
    const intro = el(
      "p",
      "note",
      "Your screen as Windows screen capture returns it on this PC. Protected windows should be missing.",
    );

    const frame = el("div", "viewer-frame waiting");
    const canvas = el("canvas");
    canvas.width = 0;
    canvas.height = 0;
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", "Your screen as screen capture sees it");
    const message = el("p", "viewer-message", "Capturing…");
    message.setAttribute("role", "status");
    frame.append(canvas, message);

    const captionRow = el("div", "viewer-caption");
    const caption = el("span");
    const screen = el("span", "viewer-screen");
    captionRow.append(caption, screen);

    const caveat = el(
      "p",
      "note",
      "This shows what Windows returns. Zoom, Teams or OBS may capture differently. Use the test page to check a real share.",
    );
    const privacy = el("p", "note", "The picture stays in this window. It is not saved or sent.");

    const actions = el("div", "dialog-actions");
    const next = button("Next screen");
    next.hidden = true;
    const pause = button("Pause");
    const test = button("Open test page");
    const close = el("button", "primary", "Close");
    close.type = "submit";
    close.value = "close";
    actions.append(test, el("span", "spacer"), next, pause, close);

    form.append(title, intro, frame, captionRow, caveat, privacy, actions);
    dialog.append(form);
    document.body.append(dialog);

    pause.addEventListener("click", () => {
      viewer.paused = !viewer.paused;
      pause.textContent = viewer.paused ? "Resume" : "Pause";
      if (viewer.paused) stopViewer();
      else captureNow();
      updateCaption();
    });
    next.addEventListener("click", () => {
      viewer.monitor = ((viewer.monitor ?? 0) + 1) % Math.max(1, viewer.count);
      captureNow(true); // one picture of the new screen, even while paused
    });
    test.addEventListener("click", () => run(() => invoke("open_test_page")));
    dialog.addEventListener("close", () => {
      stopViewer();
      viewer.generation++;
      viewer.busy = false;
      // Drop the picture as soon as the dialog closes; nothing of it is kept.
      canvas.width = 0;
      canvas.height = 0;
      viewer.hasFrame = false;
    });

    Object.assign(viewer, { dialog, canvas, frame, message, caption, screen, pause, next });
  }

  function stopViewer() {
    clearTimeout(viewer.timer);
    viewer.timer = 0;
  }

  const viewerLive = () => viewer.dialog?.open && !viewer.paused && !document.hidden;

  /** Capture right away; `once` allows a single capture while paused. */
  function captureNow(once = false) {
    stopViewer();
    if (viewer.busy) viewer.again = true;
    else if (viewerLive() || (once && viewer.dialog?.open && !document.hidden)) capture();
  }

  /** Turn whatever the IPC layer handed back into bytes. */
  function toBytes(result) {
    if (result instanceof ArrayBuffer) return new Uint8Array(result);
    if (ArrayBuffer.isView(result)) return new Uint8Array(result.buffer, result.byteOffset, result.byteLength);
    if (Array.isArray(result)) return Uint8Array.from(result);
    throw new Error("the picture arrived in an unexpected form");
  }

  async function capture() {
    const generation = viewer.generation;
    viewer.timer = 0;
    viewer.busy = true;
    viewer.again = false;
    try {
      const width = viewer.frame.clientWidth * (window.devicePixelRatio || 1);
      const maxWidth = Math.max(320, Math.min(960, Math.round(width) || 640));
      const args = { maxWidth };
      if (viewer.monitor !== undefined) args.monitor = viewer.monitor;
      const bytes = toBytes(await invoke("viewer_frame", args));
      if (generation !== viewer.generation) return;
      draw(bytes);
    } catch (error) {
      if (generation !== viewer.generation) return;
      showViewerError(error);
    } finally {
      if (generation === viewer.generation) {
        viewer.busy = false;
        // Wait for each frame before asking for the next, so frames never pile up.
        const wanted = viewer.again && viewer.dialog?.open && !document.hidden;
        if (viewerLive() || wanted) viewer.timer = setTimeout(capture, viewer.again ? 0 : 1000);
      }
    }
  }

  function draw(bytes) {
    if (bytes.byteLength < 16) throw new Error("the picture was empty");
    const view = new DataView(bytes.buffer, bytes.byteOffset, 16);
    const width = view.getUint32(0, true);
    const height = view.getUint32(4, true);
    const index = view.getUint32(8, true);
    const count = view.getUint32(12, true);
    const size = width * height * 4;
    if (!width || !height || bytes.byteLength < 16 + size) throw new Error("the picture was incomplete");

    const pixels = new Uint8ClampedArray(bytes.buffer, bytes.byteOffset + 16, size);
    const { canvas } = viewer;
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    canvas.getContext("2d").putImageData(new ImageData(pixels, width, height), 0, 0);

    viewer.monitor = index;
    viewer.count = Math.max(1, count);
    viewer.hasFrame = true;
    viewer.frame.classList.remove("waiting", "failed");
    viewer.message.textContent = "";
    viewer.message.hidden = true;
    updateCaption();
  }

  function showViewerError(error) {
    const reason = String(error?.message ?? error).replace(/[.\s]+$/, "");
    viewer.canvas.width = 0;
    viewer.canvas.height = 0;
    viewer.hasFrame = false;
    viewer.frame.classList.add("waiting", "failed");
    viewer.message.hidden = false;
    viewer.message.textContent = `Couldn't capture the screen: ${reason}.`;
    updateCaption();
  }

  function updateCaption() {
    if (!viewer.dialog) return;
    // Only windows Hidely protects; minimised or off-screen ones may not be in the picture.
    const hidden = windows().filter((w) => w.status === "applied" && w.owned).length;
    viewer.caption.textContent =
      hidden === 1
        ? "1 window is protected. It should be missing here."
        : hidden > 1
          ? `${hidden} windows are protected. They should be missing here.`
          : "Nothing is hidden yet, so this matches your screen. Protect a window and it disappears from this picture.";
    viewer.caption.parentElement.classList.toggle("empty", hidden === 0);
    const multi = viewer.count > 1;
    viewer.next.hidden = !multi;
    const parts = [];
    if (multi && viewer.hasFrame) parts.push(`Screen ${viewer.monitor + 1} of ${viewer.count}`);
    if (viewer.paused) parts.push("Paused");
    viewer.screen.textContent = parts.join(" · ");
  }

  function openViewer() {
    if (!viewer.dialog) return;
    if (viewer.dialog.open) return;
    // The palette is the only dialog it may replace; others stay and this opens on top.
    if (palette.dialog?.open) palette.dialog.close();
    viewer.paused = false;
    viewer.again = false;
    viewer.hasFrame = false;
    viewer.pause.textContent = "Pause";
    viewer.frame.classList.add("waiting");
    viewer.frame.classList.remove("failed");
    viewer.message.hidden = false;
    viewer.message.textContent = "Capturing…";
    updateCaption();
    viewer.dialog.showModal();
    captureNow();
  }

  safe("viewer", () => {
    buildViewer();
    $("open-viewer")?.addEventListener("click", openViewer);
    listen("open-viewer", () => openViewer());
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) stopViewer();
      else if (viewerLive() && !viewer.busy && !viewer.timer) capture();
    });
    // The caption counts protected windows, which change between frames.
    renderers.push(() => {
      if (viewer.dialog?.open) updateCaption();
    });
  });

  // ================================================================ Ctrl+K palette

  const palette = {
    dialog: null,
    input: null,
    list: null,
    empty: null,
    commands: [],
    shown: [],
    active: 0,
    chosen: null, // command to run once the palette has closed
  };

  function buildPalette() {
    const dialog = el("dialog", "palette");
    dialog.id = "palette";
    dialog.setAttribute("aria-label", "Commands");
    const box = el("div", "palette-box");
    const input = el("input");
    input.id = "palette-input";
    input.type = "text";
    input.placeholder = "Type a command";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.autofocus = true;
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-expanded", "true");
    input.setAttribute("aria-controls", "palette-list");
    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-label", "Command");
    const list = el("ul");
    list.id = "palette-list";
    list.setAttribute("role", "listbox");
    list.setAttribute("aria-label", "Commands");
    const empty = el("p", "palette-empty", "No commands match.");
    empty.hidden = true;
    box.append(input, list, empty);
    dialog.append(box);
    document.body.append(dialog);

    input.addEventListener("input", () => {
      palette.active = 0;
      drawPalette();
    });
    input.addEventListener("keydown", (event) => {
      const last = palette.shown.length - 1;
      if (event.key === "ArrowDown") palette.active = Math.min(last, palette.active + 1);
      else if (event.key === "ArrowUp") palette.active = Math.max(0, palette.active - 1);
      else if (event.key === "PageDown") palette.active = Math.min(last, palette.active + 6);
      else if (event.key === "PageUp") palette.active = Math.max(0, palette.active - 6);
      else if (event.key === "Enter") {
        event.preventDefault();
        choose(palette.shown[palette.active]);
        return;
      } else return;
      event.preventDefault();
      markActive();
    });
    // Keep focus in the input while the mouse picks an option.
    list.addEventListener("mousedown", (event) => event.preventDefault());
    list.addEventListener("click", (event) => {
      const item = event.target.closest("[role=option]");
      if (item) choose(palette.shown[Number(item.dataset.index)]);
    });
    list.addEventListener("mousemove", (event) => {
      const item = event.target.closest("[role=option]");
      const index = item ? Number(item.dataset.index) : -1;
      if (index >= 0 && index !== palette.active) {
        palette.active = index;
        markActive(false);
      }
    });
    // A click outside the box lands on the dialog itself (its backdrop).
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) dialog.close();
    });
    dialog.addEventListener("close", () => {
      const command = palette.chosen;
      palette.chosen = null;
      // Back to the list first, so a command that moves focus (search, a dialog) wins.
      focusList();
      if (command) safe("command", () => command.run());
    });

    Object.assign(palette, { dialog, input, list, empty });
  }

  function choose(command) {
    if (!command) return;
    palette.chosen = command;
    palette.dialog.close();
  }

  const click = (id) => () => $(id)?.click();
  const exists = (id) => {
    const node = $(id);
    return Boolean(node) && !node.disabled;
  };

  /** Every command that can run right now, in display order. */
  function buildCommands() {
    const m = model();
    const commands = [];
    const add = (label, run, keys = "", extra = {}) => commands.push({ label, run, keys, ...extra });
    if (!m) {
      if (viewer.dialog) add("What viewers see", openViewer);
      return commands;
    }
    const active = session();
    const list = windows();

    if (exists("session-button")) {
      add(active ? "End privacy session" : "Start privacy session", click("session-button"), shortcutKeys("toggle"));
    }
    if (active || list.some((w) => w.owned)) {
      add(
        "Make everything capturable again",
        () => run(() => invoke("end_session")),
        shortcutKeys("restore"),
        { aliases: "restore all windows unprotect" },
      );
    }

    const hwnd = H.selected;
    const data = hwnd ? H.rowData?.(hwnd) : undefined;
    if (data) {
      const byApp = data.status === "applied" && !data.owned;
      if (!byApp && data.status !== "applying" && typeof H.toggleRow === "function") {
        const label = data.owned
          ? "Unprotect selected window"
          : data.status === "error"
            ? "Retry protecting selected window"
            : "Protect selected window";
        add(label, () => H.toggleRow(hwnd), "Enter", { detail: data.title });
      }
      if (typeof H.togglePin === "function") {
        add(data.pinned ? "Unpin selected window" : "Pin selected window", () => H.togglePin(hwnd), "P", {
          detail: data.title,
        });
      }
    }

    if (viewer.dialog) add("What viewers see", openViewer);

    const selected = m.selected_profile ?? null;
    for (const profile of m.profiles ?? []) {
      if (profile.id === selected) continue;
      add(`Switch profile: ${profile.name}`, () => run(() => invoke("select_profile", { id: profile.id })));
    }
    if (selected !== null) {
      add("Switch profile: No profile", () => run(() => invoke("select_profile", { id: null })));
    }
    if (exists("new-profile")) add("New profile", click("new-profile"));
    for (const profile of m.profiles ?? []) {
      const edit = [...document.querySelectorAll("#profiles button")].find(
        (b) => b.getAttribute("aria-label") === `Edit profile ${profile.name}`,
      );
      if (edit) add(`Edit profile: ${profile.name}`, () => edit.click());
    }

    const filters = [
      ["all", "Show all windows"],
      ["pinned", "Show pinned windows"],
      ["protected", "Show protected windows"],
      ["problems", "Show windows that need attention"],
    ];
    for (const [name, label] of filters) {
      const filterButton = document.querySelector(`.filter[data-filter="${name}"]`);
      if (filterButton) add(label, () => filterButton.click(), "", { aliases: "filter" });
    }
    const search = $("search");
    if (search) {
      add(
        "Search windows",
        () => {
          search.focus();
          search.select();
        },
        "/",
        { aliases: "find" },
      );
    }

    if (exists("open-settings")) add("Settings", click("open-settings"), "", { aliases: "preferences options" });
    if (exists("open-shortcuts")) add("Change shortcuts", click("open-shortcuts"), "", { aliases: "keys hotkeys" });
    if (exists("open-diagnostics")) add("Diagnostics", click("open-diagnostics"), "", { aliases: "report log" });
    if (typeof m.meeting_detection === "boolean") {
      add(
        m.meeting_detection ? "Stop offering a session when a meeting app opens" : "Offer a session when a meeting app opens",
        () => run(() => invoke("set_meeting_detection", { enabled: !m.meeting_detection })),
        "",
        { aliases: "meeting zoom teams" },
      );
    }
    if (m.first_run_done === true && $("replay-tour")) add("Show the first-run tour again", replayTour);
    if (m.elevated === false && $("restart-elevated")) {
      add("Restart as administrator", click("restart-elevated"), "", { aliases: "elevated admin" });
    }
    if (exists("open-settings") && exists("check-updates")) {
      add("Check for updates", () => {
        $("open-settings").click();
        $("check-updates")?.click();
      });
    }
    return commands;
  }

  /** Substring match with a boost for the start of the name or of a word. No fuzzy matching. */
  function score(command, query) {
    if (!query) return 1;
    const name = command.label.toLowerCase();
    const words = name.split(/[^a-z0-9]+/).filter(Boolean);
    if (name.startsWith(query)) return 3;
    if (words.some((w) => w.startsWith(query))) return 2;
    if (name.includes(query)) return 1;
    const aliases = (command.aliases ?? "").split(" ");
    if (aliases.some((w) => w.startsWith(query))) return 1;
    return 0;
  }

  function drawPalette() {
    const query = palette.input.value.trim().toLowerCase();
    palette.shown = palette.commands
      .map((command, index) => ({ command, index, score: score(command, query) }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score || a.index - b.index)
      .map((entry) => entry.command);
    palette.active = Math.min(palette.active, Math.max(0, palette.shown.length - 1));

    palette.list.replaceChildren(
      ...palette.shown.map((command, index) => {
        const item = el("li", "palette-item");
        item.id = `pc-${index}`;
        item.dataset.index = String(index);
        item.setAttribute("role", "option");
        const label = el("span", "palette-label");
        label.append(el("span", "", command.label));
        if (command.detail) label.append(el("span", "palette-detail", command.detail));
        item.append(label);
        if (command.keys) item.append(kbds(command.keys));
        return item;
      }),
    );
    palette.empty.hidden = palette.shown.length > 0;
    palette.list.hidden = palette.shown.length === 0;
    markActive();
  }

  function markActive(scroll = true) {
    const items = palette.list.children;
    for (let i = 0; i < items.length; i++) items[i].setAttribute("aria-selected", String(i === palette.active));
    const current = items[palette.active];
    if (current) {
      palette.input.setAttribute("aria-activedescendant", current.id);
      if (scroll) current.scrollIntoView({ block: "nearest" });
    } else {
      palette.input.removeAttribute("aria-activedescendant");
    }
  }

  function openPalette() {
    palette.commands = buildCommands();
    palette.input.value = "";
    palette.active = 0;
    palette.chosen = null;
    drawPalette();
    palette.dialog.showModal();
    palette.input.focus();
  }

  safe("palette", () => {
    buildPalette();
    document.addEventListener("keydown", (event) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
      if (event.key.toLowerCase() !== "k") return;
      // Another dialog owns the keys (the shortcut recorder captures them).
      if (anyModalOpen(palette.dialog)) return;
      event.preventDefault();
      if (palette.dialog.open) palette.dialog.close();
      else openPalette();
    });
  });

  // ==================================================================== wiring

  safe("render hook", () => {
    if (typeof H.onRender === "function") H.onRender(onSnapshot);
    // main.js may already have rendered before this file loaded.
    if (H.model) onSnapshot(H.model);
  });
})();
