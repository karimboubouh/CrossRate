import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// Apply the pinned appearance before the first paint so there is no flash of
// the wrong theme while settings load.
try {
  const pinned = window.localStorage.getItem("crossrate.theme");
  if (pinned === "light" || pinned === "dark") {
    document.documentElement.dataset.theme = pinned;
  }
} catch {
  /* storage unavailable: fall back to the system appearance */
}

// Inside Tauri the webview is already the right size; in a plain browser (local
// design preview only) pin the panel to the real 408x724 window.
if (!("__TAURI_INTERNALS__" in window)) {
  document.documentElement.classList.add("preview");
}

async function boot(): Promise<void> {
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    const [{ installPreview }, { api }] = await Promise.all([
      import("./lib/preview"),
      import("./lib/api"),
    ]);
    installPreview(api);
  }

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
