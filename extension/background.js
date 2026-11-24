// Hidely for browsers: tells the Hidely app when a private tab is showing.
//
// Screen capture can only leave out whole windows, not single tabs. So this extension
// watches which tab is active in each browser window, and while that tab is one you marked
// private (or is on a site you marked private) it asks Hidely to keep that whole window
// out of capture. Switch to an ordinary tab and the window is released again.
//
// Nothing leaves your computer: the only thing this talks to is the Hidely app, and all it
// sends is the active tab's title and the window's position, so Hidely can find the window.

const HOST = "io.github.codingrule.hidely";

let port = null;
let nextId = 1;
const waiting = new Map(); // request id -> resolve
let link = { connected: false, running: false, error: null };
const lastAsked = new Map(); // window id -> { protect, title }
const windowState = new Map(); // window id -> { protected, error }

// ------------------------------------------------------------------- storage

/** Tabs marked private for this browser session. Forgotten when the browser closes. */
async function privateTabs() {
  return new Set((await chrome.storage.session.get({ tabs: [] })).tabs);
}

/** Sites that are always private, as host names such as "mail.example.com". */
async function privateSites() {
  return (await chrome.storage.local.get({ sites: [] })).sites;
}

function hostOf(url) {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.hostname : null;
  } catch {
    return null;
  }
}

/** A site entry covers that host and its subdomains: "example.com" covers "mail.example.com". */
function siteCovers(site, host) {
  return host === site || host.endsWith(`.${site}`);
}

async function isPrivate(tab) {
  if ((await privateTabs()).has(tab.id)) return true;
  const host = hostOf(tab.url ?? "");
  return host !== null && (await privateSites()).some((site) => siteCovers(site, host));
}

// ---------------------------------------------------------------- Hidely link

function connect() {
  if (port) return;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch (error) {
    link = { connected: false, running: false, error: String(error) };
    return;
  }
  link = { connected: true, running: link.running, error: null };
  port.onMessage.addListener((message) => {
    if ("running" in message) link.running = message.running;
    waiting.get(message.id)?.(message);
    waiting.delete(message.id);
  });
  port.onDisconnect.addListener(() => {
    // Chrome sets lastError when the host could not be started at all, which means the
    // app has not been connected to this browser yet.
    const reason = chrome.runtime.lastError?.message ?? null;
    port = null;
    link = { connected: false, running: false, error: reason };
    for (const resolve of waiting.values()) resolve({ ok: false, error: "The connection to Hidely closed." });
    waiting.clear();
    lastAsked.clear();
    windowState.clear();
  });
}

function ask(message) {
  connect();
  if (!port) return Promise.resolve({ ok: false, error: link.error ?? "Hidely is not connected." });
  return new Promise((resolve) => {
    const id = nextId++;
    waiting.set(id, resolve);
    port.postMessage({ ...message, id });
  });
}

// --------------------------------------------------------------------- logic

/** Bring one browser window in line with its active tab. */
async function sync(windowId) {
  if (windowId === chrome.windows.WINDOW_ID_NONE) return;
  let browserWindow;
  try {
    browserWindow = await chrome.windows.get(windowId, { populate: true });
  } catch {
    return; // the window closed in the meantime
  }
  if (browserWindow.type !== "normal") return;
  const tab = browserWindow.tabs.find((candidate) => candidate.active);
  if (!tab) return;

  const protect = await isPrivate(tab);
  const previous = lastAsked.get(windowId);
  // Nothing to do if we already asked for this, unless it failed last time.
  const failed = windowState.get(windowId)?.error;
  if (previous && previous.protect === protect && !failed) return;
  if (!protect && !previous?.protect) {
    lastAsked.set(windowId, { protect: false });
    return; // never protected, so nothing to release
  }

  lastAsked.set(windowId, { protect });
  const reply = await ask({
    type: "set",
    windowId,
    protect,
    title: tab.title ?? "",
    bounds: {
      left: browserWindow.left,
      top: browserWindow.top,
      width: browserWindow.width,
      height: browserWindow.height,
    },
  });
  windowState.set(windowId, {
    protected: reply.protected === true,
    error: reply.ok ? null : (reply.error ?? "Hidely did not answer."),
  });
  updateBadge(windowId);
}

async function syncAll() {
  for (const browserWindow of await chrome.windows.getAll({ windowTypes: ["normal"] })) {
    await sync(browserWindow.id);
  }
}

/** A small mark on the toolbar icon: "on" while the window is protected, "!" on a problem. */
async function updateBadge(windowId) {
  const state = windowState.get(windowId);
  const text = state?.error ? "!" : state?.protected ? "on" : "";
  const color = state?.error ? "#b3261e" : "#17874a";
  for (const tab of await chrome.tabs.query({ windowId })) {
    chrome.action.setBadgeText({ tabId: tab.id, text });
    chrome.action.setBadgeBackgroundColor({ tabId: tab.id, color });
  }
}

// -------------------------------------------------------------------- events

chrome.tabs.onActivated.addListener(({ windowId }) => sync(windowId));

chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  // A new address can move the tab on to, or off, a private site. A new title matters
  // while Hidely is still looking for the window.
  if (tab.active && (change.url || change.title || change.status === "complete")) sync(tab.windowId);
});

chrome.tabs.onRemoved.addListener(async (tabId) => {
  const tabs = await privateTabs();
  if (tabs.delete(tabId)) await chrome.storage.session.set({ tabs: [...tabs] });
});

chrome.tabs.onAttached.addListener((tabId, { newWindowId }) => sync(newWindowId));
chrome.tabs.onDetached.addListener((tabId, { oldWindowId }) => sync(oldWindowId));

chrome.windows.onRemoved.addListener((windowId) => {
  if (lastAsked.get(windowId)?.protect) ask({ type: "forget", windowId });
  lastAsked.delete(windowId);
  windowState.delete(windowId);
});

chrome.runtime.onStartup.addListener(syncAll);
chrome.runtime.onInstalled.addListener(syncAll);

// ----------------------------------------------------------- popup requests

async function describe(tab) {
  const host = hostOf(tab.url ?? "");
  const sites = await privateSites();
  const hello = await ask({ type: "hello" });
  return {
    host,
    tabPrivate: (await privateTabs()).has(tab.id),
    sitePrivate: host !== null && sites.some((site) => siteCovers(site, host)),
    sites,
    window: windowState.get(tab.windowId) ?? { protected: false, error: null },
    hidely: {
      connected: link.connected && hello.ok === true,
      running: hello.running === true,
      version: hello.version ?? null,
      error: hello.ok ? null : (link.error ?? hello.error ?? null),
    },
  };
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  (async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (!tab) return respond(null);

    if (message.type === "set-tab") {
      const tabs = await privateTabs();
      if (message.private) tabs.add(tab.id);
      else tabs.delete(tab.id);
      await chrome.storage.session.set({ tabs: [...tabs] });
    } else if (message.type === "set-site") {
      const host = message.host ?? hostOf(tab.url ?? "");
      if (host) {
        const sites = (await privateSites()).filter((site) => site !== host);
        if (message.private) sites.push(host);
        await chrome.storage.local.set({ sites: sites.sort() });
      }
    }
    if (message.type !== "state") {
      // Settings changed, so every window may need to change with them.
      lastAsked.clear();
      await syncAll();
    }
    respond(await describe(tab));
  })();
  return true; // the answer comes later
});
