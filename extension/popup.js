const $ = (id) => document.getElementById(id);

function render(state) {
  if (!state) {
    $("link").textContent = "No tab";
    return;
  }

  // The connection to the Hidely app.
  const link = $("link");
  const problem = $("problem");
  problem.hidden = true;
  if (!state.hidely.connected) {
    link.textContent = "Not connected";
    link.className = "link bad";
    problem.textContent =
      "This browser is not connected to Hidely yet. Open Hidely, go to Settings, and switch on \"Connect Chrome and Edge\". Then reopen this.";
    problem.hidden = false;
  } else if (!state.hidely.running) {
    link.textContent = "Hidely is closed";
    link.className = "link bad";
    problem.textContent = "Hidely is not running, so nothing is being protected. Start Hidely.";
    problem.hidden = false;
  } else {
    link.textContent = `Connected · ${state.hidely.version}`;
    link.className = "link ok";
  }

  // The two choices for the tab in front.
  $("tab").checked = state.tabPrivate;
  $("site").checked = state.sitePrivate;
  $("site").disabled = !state.host;
  $("site-label").textContent = state.host
    ? `Always keep ${state.host} private`
    : "This page can't be added as a site";

  // What is actually happening to this window, in words.
  const status = $("status");
  const wanted = state.tabPrivate || state.sitePrivate;
  if (state.window.error) {
    status.textContent = state.window.error;
    status.className = "status bad";
  } else if (state.window.protected) {
    status.textContent = "This window is being kept out of screen capture.";
    status.className = "status ok";
  } else if (wanted) {
    status.textContent = "Not protected yet.";
    status.className = "status bad";
  } else {
    status.textContent = "This window is captured normally.";
    status.className = "status";
  }

  // The saved sites, each with a way to remove it.
  $("sites-section").hidden = state.sites.length === 0;
  $("sites").replaceChildren(
    ...state.sites.map((site) => {
      const item = document.createElement("li");
      const name = document.createElement("span");
      name.textContent = site;
      const remove = document.createElement("button");
      remove.type = "button";
      remove.textContent = "Remove";
      remove.setAttribute("aria-label", `Stop keeping ${site} private`);
      remove.addEventListener("click", () => send({ type: "set-site", host: site, private: false }));
      item.append(name, remove);
      return item;
    }),
  );
}

async function send(message) {
  render(await chrome.runtime.sendMessage(message));
}

$("tab").addEventListener("change", (event) => send({ type: "set-tab", private: event.target.checked }));
$("site").addEventListener("change", (event) => send({ type: "set-site", private: event.target.checked }));
send({ type: "state" });
