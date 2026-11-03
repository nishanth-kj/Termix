# Building

Everyone needs:

- Rust 1.95 or newer (`rustup`) and a C compiler.
- CMake and, on Windows, [NASM](https://www.nasm.us/), for the `aws-lc` crypto library. For a debug build without NASM, set `AWS_LC_SYS_NO_ASM=1`.
- Node.js 24 or newer, for the TypeScript UI.
- Tauri CLI 2 for the app: `cargo install tauri-cli --version "^2" --locked`.

The CLI and libraries need nothing else:

```bash
cargo test
cargo build -p termix-cli
```

The app embeds the built UI, so build the UI before compiling `termix-app` directly. `cargo tauri` does this for you.

```bash
cd app
npm ci
npm run build
```

## Desktop

- Windows: WebView2, included with Windows 11.
- macOS: Xcode command line tools.
- Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`.

From `app/`, run `cargo tauri dev`, or `cargo tauri build` for an installer.

## Android

Android Studio's SDK, NDK 27, a JDK, and the Rust targets (`rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`). Set `ANDROID_HOME`, `NDK_HOME`, and `ANDROID_NDK_ROOT`.

On Windows, also put the SDK's `cmake/<version>/bin` on `PATH` and set `CMAKE_GENERATOR=Ninja`. Otherwise CMake picks Visual Studio, which cannot build the crypto library for Android.

The Android project is committed in `app/src-tauri/gen/android`. Every class in this app is Java, in `app/src/main/java/com/termix/app/`. `src/main/java/**/generated` is excluded from compilation: an Android Rust build may write Kotlin there, and `stripGeneratedKotlin` deletes it. Do not re-run `cargo tauri android init`. Gradle 8 needs JDK 17 or 21, not 25.

```bash
cd app
cargo tauri android dev
cargo tauri android build --debug --apk
```

## iOS

A Mac with Xcode. From `app/`, run `cargo tauri ios init` once, then `cargo tauri ios dev`. Installing on a device needs an Apple developer account.

## Tests

```bash
cargo test --workspace
```

Tests use an in-process SSH and SFTP server, so no machine is needed. To try the app or CLI against that server, run `cargo run -p termix-session --example test_server`.

On Windows, this drives the real window and CLI using temporary data:

```bash
npm --prefix app ci && npm --prefix app run build:e2e
cargo build -p termix-cli -p termix-app
cargo build -p termix-session --example test_server
node scripts/e2e-windows.mts screenshots
```

`build:e2e` adds test hooks, so rebuild with `npm run build` afterwards.

CI compiles and tests every desktop platform. Its bundles are unsigned: signing needs certificates and developer accounts this repository does not have.
