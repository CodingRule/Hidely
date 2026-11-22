window.__TAURI__.event.listen("hud", (event) => {
  document.getElementById("message").textContent = event.payload;
});
