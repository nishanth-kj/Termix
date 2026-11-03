# Building

Requirements:

- Rust 1.85 or newer (`rustup`).
- A C compiler (SQLite is bundled).
- For the desktop UI: the Dioxus CLI and a system webview.
  - Linux: WebKitGTK (`libwebkit2gtk`).
  - Windows: WebView2.
  - macOS: system WebKit.
- iOS: macOS, Xcode, iOS SDK.
- Android: Android SDK, NDK, JDK.

```bash
cargo test
cargo build -p termix-cli
```

UI:

```bash
cargo install dioxus-cli
cd app
dx serve --desktop
```

`dx bundle` produces `.exe` / `.msi`, `.app` / `.dmg`, `.apk`, and an iOS `.app` when run on a machine with that SDK. This repository's CI compiles and tests the Rust crates. It does not claim to have produced store binaries.
