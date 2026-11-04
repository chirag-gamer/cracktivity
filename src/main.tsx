import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { SystemThemeProvider } from "./shared/theme";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SystemThemeProvider>
      <App />
    </SystemThemeProvider>
  </React.StrictMode>,
);
