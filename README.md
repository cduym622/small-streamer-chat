<img src="assets/icon.svg" width="64" height="64" alt="">

# Small Streamer Chat

An AI Practice Chat for small streamers, so you always have someone to talk to. Work in progress: see [docs/mvp-plan.md](docs/mvp-plan.md).

## Development

Prerequisites: the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Rust, plus the Xcode command line tools on macOS or the MSVC build tools on Windows), Node.js LTS, and [CMake](https://cmake.org/download/) for building whisper.cpp (`brew install cmake` on macOS; on Windows, tick "C++ CMake tools for Windows" in the Visual Studio Build Tools installer).

The first launch downloads the speech model (~60 MB) into the app's data folder.

```sh
npm install
npm run tauri dev     # run the app
npm run tauri build   # build installers for this OS
```

## Releases

GitHub Actions builds the Windows (`-setup.exe`) and macOS (`.dmg`, Apple Silicon and Intel) installers. Push a version tag such as `v0.1.0` and a draft GitHub Release is created with the installers attached. Builds from pull requests and manual runs are attached to the workflow run as artifacts.
