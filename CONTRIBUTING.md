# Contributing to GladiaFlow

Thanks for helping improve GladiaFlow. Bug fixes, platform work, documentation, and focused feature proposals are all welcome.

## Before you start

- Read the [README](README.md) for product overview, setup, and architecture.
- For larger changes, [open an issue](https://github.com/gladiaio/gladiaflow/issues) first so the design can be discussed before implementation.
- On macOS, Accessibility grants are tied to the app's code-signing identity. Changing the release signing certificate can require users to re-grant Accessibility.

## Development setup

1. Install [Node.js](https://nodejs.org/) 20.19+, [Rust](https://www.rust-lang.org/tools/install) stable, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.
2. Clone the repo, install dependencies, and start the desktop app:

```bash
git clone https://github.com/gladiaio/gladiaflow.git
cd gladiaflow
npm ci
npm run tauri:dev
```

3. Enter a [Gladia API key](https://app.gladia.io/) in the onboarding screen when testing transcription end to end. Real microphone audio is streamed to Gladia under their [privacy notice](https://www.gladia.io/privacy-notice) and [terms](https://www.gladia.io/terms-conditions); see [Privacy and data handling](README.md#privacy-and-data-handling).

`npm run dev` is enough for UI-only work. Native audio, global shortcuts, clipboard paste, and Tauri commands require `npm run tauri:dev`.

## Making changes

1. Fork the repository and create a branch from `main`.
2. Keep the diff focused on one problem or feature.
3. Prefer tests when behavior changes:
   - Rust: `cargo test --manifest-path src-tauri/Cargo.toml`
   - Frontend / full suite: `npm test`
4. Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages, for example `fix: restore clipboard after dictation`.
5. Before opening a pull request, run:

```bash
npm run format
npm test
npm run build
```

For capture or lifecycle changes, also run:

```bash
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

## Pull requests

Open a PR that explains:

- the problem
- the approach
- how you tested it (platforms, devices, and permission states when relevant)

Maintainers may ask for follow-up on hotkey races, prepared-microphone lifecycle, Accessibility grant flow, or paste/clipboard behavior — those paths are easy to regress.

## Security

Please do not open public issues for security vulnerabilities. See [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE). Brand and logo use is governed by [TRADEMARKS.md](TRADEMARKS.md), not the MIT license.
