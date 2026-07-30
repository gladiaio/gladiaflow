import React from "react";
import ReactDOM from "react-dom/client";
import "../gladia-tokens.css";
import "../index.css";
import { ScreenshotGallery } from "./Gallery";

const style = document.createElement("style");
style.textContent = `
  body {
    overflow: auto;
    height: auto;
    background: #111;
  }
  #root {
    height: auto;
  }
  .screenshot-gallery {
    display: flex;
    flex-direction: column;
    gap: 32px;
    padding: 32px;
  }
  .screenshot-frame-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .screenshot-label {
    color: #909090;
    font-family: var(--font-sans);
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .screenshot-frame-wrap .layout {
    border-radius: 8px;
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.08);
  }
`;
document.head.appendChild(style);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ScreenshotGallery />
  </React.StrictMode>,
);
