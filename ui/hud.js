// The backend sends the words to show. A problem ("Not protected: …", "… is visible to
// viewers again") gets the red look, so it cannot be mistaken for good news. The payload is
// a plain string, or an object { message, problem } if the backend ever says so itself.
const PROBLEM = /not protected|visible to viewers|could not|couldn't|lost protection/i;

window.__TAURI__.event.listen("hud", (event) => {
  const payload = event.payload;
  const text = typeof payload === "string" ? payload : String(payload?.message ?? payload?.text ?? "");
  const problem = typeof payload === "string" ? PROBLEM.test(text) : Boolean(payload?.problem);
  const message = document.getElementById("message");
  message.textContent = text;
  message.classList.toggle("problem", problem);
});
