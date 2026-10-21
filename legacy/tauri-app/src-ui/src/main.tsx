import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";
import { installBrowserTauriHarness } from "./browserTauriHarness";
import { isBrowserHarnessRuntime } from "./runtimeMode";

if (isBrowserHarnessRuntime()) {
  installBrowserTauriHarness();
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
