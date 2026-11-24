// Hidely site: the tilting app, the split desktop, the star count, and the screen-share test.

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const finePointer = window.matchMedia("(hover: hover) and (pointer: fine)").matches;

// ------------------------------------------------------------ the tilting app

const app = document.getElementById("app");
if (app && finePointer && !reduceMotion) {
  const MAX_X = 10; // degrees around the horizontal axis
  const MAX_Y = 13; // degrees around the vertical axis
  let frame = 0;

  app.addEventListener("pointermove", (event) => {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const box = app.getBoundingClientRect();
      const x = (event.clientX - box.left) / box.width; // 0 at the left edge, 1 at the right
      const y = (event.clientY - box.top) / box.height;
      // The side under the pointer dips away, as if pressed.
      app.style.setProperty("--ry", `${((x - 0.5) * 2 * MAX_Y).toFixed(2)}deg`);
      app.style.setProperty("--rx", `${((0.5 - y) * 2 * MAX_X).toFixed(2)}deg`);
      app.style.setProperty("--gx", `${(x * 100).toFixed(1)}%`);
      app.style.setProperty("--gy", `${(y * 100).toFixed(1)}%`);
      app.classList.add("live");
    });
  });

  app.addEventListener("pointerleave", () => {
    cancelAnimationFrame(frame);
    app.classList.remove("live");
    app.style.setProperty("--rx", "0deg");
    app.style.setProperty("--ry", "0deg");
  });
}

// ------------------------------------------- app toggles drive the viewer side

const theirs = document.getElementById("theirs");
const summary = document.getElementById("app-summary");
const pill = document.getElementById("app-pill");

function render() {
  let count = 0;
  for (const row of document.querySelectorAll(".row")) {
    const protectedNow = row.querySelector(".toggle").getAttribute("aria-pressed") === "true";
    row.classList.toggle("on", protectedNow);
    row.querySelector(".status").textContent = protectedNow ? "Protection applied" : "Unprotected";
    row.querySelector(".toggle").textContent = protectedNow ? "Unprotect" : "Protect";
    theirs?.querySelector(`.win[data-win="${row.dataset.win}"]`)?.classList.toggle("gone", protectedNow);
    if (protectedNow) count++;
  }
  summary.textContent = count ? `Session on · ${count} protected` : "No privacy session";
  pill.classList.toggle("idle", count === 0);
}

for (const button of document.querySelectorAll(".row .toggle")) {
  button.addEventListener("click", () => {
    const next = button.getAttribute("aria-pressed") !== "true";
    button.setAttribute("aria-pressed", String(next));
    render();
  });
}
render();

// ----------------------------------------------------------------- the divider

const stage = document.getElementById("stage");
const divider = document.getElementById("divider");

function setSplit(percent) {
  const value = Math.min(95, Math.max(5, percent));
  stage.style.setProperty("--split", `${value}%`);
  divider.setAttribute("aria-valuenow", String(Math.round(value)));
  divider.setAttribute("aria-valuetext", `${Math.round(value)}% your screen`);
}

if (stage && divider) {
  const fromEvent = (event) => {
    const box = stage.getBoundingClientRect();
    return ((event.clientX - box.left) / box.width) * 100;
  };

  divider.addEventListener("pointerdown", (event) => {
    divider.setPointerCapture(event.pointerId);
    divider.classList.add("dragging");
    setSplit(fromEvent(event));
  });
  divider.addEventListener("pointermove", (event) => {
    if (divider.hasPointerCapture(event.pointerId)) setSplit(fromEvent(event));
  });
  const release = (event) => {
    divider.classList.remove("dragging");
    if (divider.hasPointerCapture(event.pointerId)) divider.releasePointerCapture(event.pointerId);
  };
  divider.addEventListener("pointerup", release);
  divider.addEventListener("pointercancel", release);

  // Clicking anywhere on the stage moves the divider there.
  stage.addEventListener("pointerdown", (event) => {
    if (event.target === divider || divider.contains(event.target)) return;
    setSplit(fromEvent(event));
  });

  divider.addEventListener("keydown", (event) => {
    const now = Number(divider.getAttribute("aria-valuenow"));
    const step = event.shiftKey ? 10 : 3;
    const moves = { ArrowLeft: -step, ArrowDown: -step, ArrowRight: step, ArrowUp: step, Home: 5 - now, End: 95 - now };
    if (event.key in moves) {
      event.preventDefault();
      setSplit(now + moves[event.key]);
    }
  });

  setSplit(50);
}

// ------------------------------------------------------------- GitHub stars

(async () => {
  const label = document.getElementById("star-count");
  if (!label) return;
  try {
    const response = await fetch("https://api.github.com/repos/CodingRule/Hidely", {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) return;
    const { stargazers_count: stars } = await response.json();
    if (typeof stars !== "number") return;
    label.textContent = stars >= 1000 ? `${(stars / 1000).toFixed(1).replace(/\.0$/, "")}k` : String(stars);
    label.closest("a")?.setAttribute("aria-label", `Hidely on GitHub, ${stars} stars`);
  } catch {
    // Offline or rate-limited: the button still says "Star".
  }
})();

// -------------------------------------------------------- screen-share test

const video = document.getElementById("preview");
const placeholder = document.getElementById("placeholder");
const status = document.getElementById("status");
const startButton = document.getElementById("start");
const stopButton = document.getElementById("stop");
const snapshotButton = document.getElementById("snapshot");
const snapshots = document.getElementById("snapshots");
let stream = null;

function setStatus(text, isError = false) {
  status.textContent = text;
  status.classList.toggle("error", isError);
}

function setCapturing(capturing) {
  video.hidden = !capturing;
  placeholder.hidden = capturing;
  startButton.disabled = capturing;
  stopButton.disabled = !capturing;
  snapshotButton.disabled = !capturing;
}

function stop() {
  if (stream) stream.getTracks().forEach((track) => track.stop());
  stream = null;
  video.srcObject = null;
  setCapturing(false);
  setStatus("Preview stopped. Nothing is being captured.");
}

startButton.addEventListener("click", async () => {
  if (!navigator.mediaDevices?.getDisplayMedia) {
    setStatus("This browser can't share the screen. Try a current Chrome, Edge or Firefox on desktop.", true);
    return;
  }
  try {
    stream = await navigator.mediaDevices.getDisplayMedia({ video: true, audio: false });
  } catch (error) {
    setStatus(
      error.name === "NotAllowedError"
        ? "Screen sharing was cancelled. Press Start screen share to try again."
        : `Couldn't start screen sharing: ${error.message}`,
      true,
    );
    return;
  }
  video.srcObject = stream;
  const track = stream.getVideoTracks()[0];
  track.addEventListener("ended", stop);
  const surface = track.getSettings().displaySurface;
  setCapturing(true);
  setStatus(
    surface && surface !== "monitor"
      ? `Previewing a single ${surface}. Pick "Entire screen" to compare windows side by side.`
      : "Previewing your screen as this browser captures it.",
  );
});

stopButton.addEventListener("click", stop);
// Leaving the page must never leave a capture running.
window.addEventListener("pagehide", () => stream?.getTracks().forEach((track) => track.stop()));

document.getElementById("test-window").addEventListener("click", () => {
  const opened = window.open("test-window.html", "hidely-test-window", "popup,width=420,height=300");
  if (!opened) setStatus("The browser blocked the test window. Allow pop-ups for this page and try again.", true);
});

snapshotButton.addEventListener("click", () => {
  if (!video.videoWidth) return;
  const canvas = document.createElement("canvas");
  canvas.width = video.videoWidth;
  canvas.height = video.videoHeight;
  canvas.getContext("2d").drawImage(video, 0, 0);
  const figure = document.createElement("figure");
  const image = document.createElement("img");
  image.src = canvas.toDataURL("image/png");
  image.alt = "Snapshot of the shared screen";
  const caption = document.createElement("figcaption");
  caption.textContent = new Date().toLocaleTimeString();
  figure.append(image, caption);
  snapshots.prepend(figure);
});
