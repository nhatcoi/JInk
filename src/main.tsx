import React from "react";
import ReactDOM from "react-dom/client";
import Popup from "./Popup";
import Settings from "./Settings";
import "./index.css";

// Simple hash routing: settings window loads index.html#/settings.
const isSettings = window.location.hash.startsWith("#/settings");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isSettings ? <Settings /> : <Popup />}</React.StrictMode>,
);
