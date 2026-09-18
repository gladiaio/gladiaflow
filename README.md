<div align="center">
  <img src="public/assets/gladiaflow-banner.png" alt="GladiaFlow — real-time voice dictation" width="100%" />

  <p><strong>Fast, accurate voice dictation in any desktop app.</strong></p>

  <p>
    <a href="https://github.com/gladiaio/gladiaflow/releases"><img src="https://img.shields.io/github/v/release/gladiaio/gladiaflow?display_name=tag" alt="Latest release" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-8b5cf6.svg" alt="MIT license" /></a>
    <img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" />
  </p>

  <p>
    <a href="#quick-start">Quick start</a> ·
    <a href="#development">Development</a> ·
    <a href="#architecture">Architecture</a> ·
    <a href="#privacy-and-data-handling">Privacy</a> ·
    <a href="#contributing">Contributing</a>
  </p>
</div>

GladiaFlow is an open-source desktop app that turns speech into text wherever you type. Hold a hotkey, speak, and the app streams your microphone to the [Gladia Live Transcription API](https://docs.gladia.io/), cleans the result, and pastes it into the focused application.

> [!NOTE]
> GladiaFlow currently supports macOS and Windows. A [Gladia API key](https://app.gladia.io/) is required.

## Why GladiaFlow?

- **Dictate anywhere** — paste transcribed speech into editors, browsers, chat apps, and more.
- **See results live** — partial and final transcripts arrive over a low-latency WebSocket session.
- **Choose your workflow** — use push-to-talk or toggle mode with a configurable global hotkey.
- **Improve specialist terms** — add custom vocabulary, pronunciations, and language hints.
- **Speak naturally** — automatic endpointing and transcript cleanup produce readable text.
- **Keep control** — manage languages, code switching, history, and dictation statistics locally.
- **Stay out of the way** — use the compact overlay and reopen the app from the system tray.

## Quick start

Download the latest installer from [GitHub Releases](https://github.com/gladiaio/gladiaflow/releases), then:

1. Launch GladiaFlow and enter your [Gladia API key](https://app.gladia.io/).
2. Grant microphone access. On macOS, also grant Accessibility access so GladiaFlow can paste into other apps.
3. Focus any text field and use the activation shortcut:
   - **macOS:** hold <kbd>Fn</kbd> / <kbd>Globe</kbd>
   - **Windows:** hold <kbd>Ctrl</kbd> + <kbd>Space</kbd>
4. Speak, then release the shortcut to finish the session.

The shortcut and activation mode can be changed in **App settings**.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) 20.19 or newer
- [Rust](https://www.rust-lang.org/tools/install) stable
- The [Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/) for your platform
- A [Gladia API key](https://app.gladia.io/) for end-to-end transcription testing

Windows development also requires the Microsoft C++ Build Tools and WebView2. WebView2 is already included with current Windows 10 and Windows 11 installations.

### Run locally

```bash
git clone https://github.com/gladiaio/gladiaflow.git
cd gladiaflow
npm ci
npm run tauri:dev
```

The last command starts Vite, opens the Tauri application, and enables frontend and Rust hot reload. Enter your API key through the onboarding screen; no `.env` file is needed.

> [!TIP]
> Run `npm run dev` when you only need the browser-based frontend. Native audio, global shortcuts, clipboard access, and Tauri commands require `npm run tauri:dev`.

### Useful commands

| Command               | Purpose                                          |
| --------------------- | ------------------------------------------------ |
| `npm run tauri:dev`   | Run the complete desktop app in development mode |
| `npm run dev`         | Run only the Vite frontend                       |
| `npm test`            | Run Rust tests, Vitest, and TypeScript checks    |
| `npm run typecheck`   | Type-check the frontend without emitting files   |
| `npm run format`      | Format the repository with Prettier              |
| `npm run build`       | Build the frontend bundle                        |
| `npm run tauri:build` | Create a production desktop bundle               |
| `npm run screenshots` | Capture the app screenshot gallery               |

Before opening a pull request, run:

```bash
npm run format
npm test
npm run build
```

## How it works

```text
Global hotkey → microphone capture → Gladia WebSocket
                                        ↓
Focused app ← clipboard-safe paste ← transcript cleanup
```

1. The global hotkey starts microphone capture and opens a Gladia live session.
2. PCM audio is sent in chunks while partial and final transcription events stream back.
3. Final utterances pass through `UtteranceCleaner`, which handles punctuation, capitalization, and duplicates.
4. Clean incremental text is pasted into the focused app without permanently replacing the clipboard.
5. Releasing the hotkey—or toggling it off—finalizes the session and returns the overlay to idle.

## Architecture

| Area            | Technology                 | Location                               |
| --------------- | -------------------------- | -------------------------------------- |
| Desktop shell   | Tauri 2                    | `src-tauri/`                           |
| Native backend  | Rust, Tokio, CPAL          | `src-tauri/src/`                       |
| Interface       | React 19, TypeScript, Vite | `src/`                                 |
| Native bridge   | Tauri commands and events  | `src/App.tsx`, `src-tauri/src/main.rs` |
| CI and releases | GitHub Actions             | `.github/workflows/`                   |

Important backend modules include:

- `gladia.rs` — live WebSocket transcription session
- `audio.rs` — microphone capture and PCM streaming
- `hotkey.rs` — global activation shortcut and accessibility checks
- `utterance_cleaner.rs` — incremental transcript post-processing
- `vocabulary.rs` — custom vocabulary expansion
- `config.rs` and `history.rs` — local settings and dictation history

The UI uses two windows: `main` for onboarding, history, and settings; and `overlay` for the floating dictation indicator.

## Platform support

| Capability           | macOS                                      | Windows                              |
| -------------------- | ------------------------------------------ | ------------------------------------ |
| Default shortcut     | <kbd>Fn</kbd> / <kbd>Globe</kbd>           | <kbd>Ctrl</kbd> + <kbd>Space</kbd>   |
| Paste integration    | CoreGraphics <kbd>Cmd</kbd> + <kbd>V</kbd> | enigo <kbd>Ctrl</kbd> + <kbd>V</kbd> |
| Clipboard            | `pbcopy` / `pbpaste`                       | `arboard`                            |
| Required permissions | Microphone and Accessibility               | Microphone                           |
| Screen-context vocab | AX + Vision OCR (Screen Recording)         | UI Automation + WinRT OCR            |
| Release bundle       | Universal DMG                              | NSIS installer                       |

Linux is not currently supported because the global hotkey, clipboard, and permission integrations are platform-specific.

## Building and releasing

Create a local production bundle with:

```bash
npm run tauri:build
```

Artifacts are written below `src-tauri/target/`. macOS universal release builds are placed in `universal-apple-darwin/release/bundle/dmg/`; Windows NSIS builds are placed in `release/bundle/nsis/`.

The release workflow runs for tags matching `v*`, stamps the tag version into the Node, Tauri, and Cargo manifests, builds macOS and Windows installers, and publishes them to GitHub Releases.

## Privacy and data handling

GladiaFlow is a local desktop client. It does not operate its own cloud backend for dictation. When you dictate, microphone audio is sent to Gladia’s Live Transcription API, which is the sole remote processor for that audio.

| Data | What happens |
| --- | --- |
| Microphone audio | During an active dictation session, PCM audio is streamed over WebSocket to the Gladia Live Transcription API. GladiaFlow does not save audio files locally. |
| Transcripts | Text returns from Gladia. Cleaned transcript text and the Gladia session ID are stored locally in SQLite (`history.db` in the app config directory). |
| Settings and API key | Stored locally in `config.json` in the user config directory (API key, hotkey, languages, custom vocabulary, and related preferences). |
| Clipboard | Used transiently to paste transcribed text into the focused app. An optional setting can leave the final transcript on the clipboard. |

Audio and transcripts processed by Gladia are subject to Gladia’s own policies:

- [Privacy notice](https://www.gladia.io/privacy-notice)
- [Terms & conditions](https://www.gladia.io/terms-conditions)

Those documents cover retention, subprocessors, and related platform practices. For how GladiaFlow stores data on your machine, see also [SECURITY.md](SECURITY.md).

## Contributing

Contributions are welcome—bug fixes, platform improvements, documentation, and focused feature proposals all help. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, checks, and pull-request expectations.

To report a security vulnerability privately, see [SECURITY.md](SECURITY.md).

## License

GladiaFlow is available under the [MIT License](LICENSE). See [TRADEMARKS.md](TRADEMARKS.md) for brand and logo restrictions.
