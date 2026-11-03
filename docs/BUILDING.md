# Building

Requirements:

- Rust 1.85 or newer (`rustup`).
- A C compiler (SQLite is bundled).
- Tauri CLI 2 for the window: `cargo install tauri-cli --version "^2" --locked`.
- A system webview:
  - Linux: WebKitGTK (`libwebkit2gtk-4.1`, GTK 3).
  - Windows: WebView2.
  - macOS: system WebKit.
- iOS: macOS, Xcode, iOS SDK.
- Android: Android SDK, NDK, JDK.

```bash
cargo test
cargo build -p termix-cli
```

UI, from `app/`:

```bash
cargo tauri dev
cargo tauri android init
cargo tauri ios init
cargo tauri android dev
cargo tauri ios dev
```

`cargo tauri build` produces the desktop bundle for the machine it runs on. Android and iOS bundles are produced only on a machine with those SDKs. CI compiles and tests the Rust crates. It does not claim to have produced store binaries.