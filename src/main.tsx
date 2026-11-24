import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// In a plain browser (design preview / screenshots) pin the panel to the real
// 408x724 window size. Inside Tauri the webview is already the right size.
if (!("__TAURI_INTERNALS__" in window)) {
  document.documentElement.classList.add("preview");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
