import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import appCss from "./index.css?inline";

function installAppStyles(css: string) {
  const existing = document.querySelector<HTMLStyleElement>("style[data-abigail-app-css]");
  if (existing) {
    existing.textContent = css;
    return;
  }

  const style = document.createElement("style");
  style.dataset.abigailAppCss = "true";
  style.textContent = css;
  document.head.appendChild(style);
}

installAppStyles(appCss);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
