import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import {
  FloatingMeetingController,
  FloatingMeetingReminder,
} from "./components/FloatingMeetingWindows";
import "./gladia-tokens.css";
import "./index.css";

const windowLabel = getCurrentWindow().label;
if (windowLabel !== "main") {
  document.documentElement.classList.add("floating-window");
  document.body.classList.add("floating-window");
}
const Surface =
  windowLabel === "meeting-controller"
    ? FloatingMeetingController
    : windowLabel === "meeting-reminder"
      ? FloatingMeetingReminder
      : App;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Surface />
  </React.StrictMode>,
);
