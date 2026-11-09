# Contributing

## Development

Built with [Tauri](https://tauri.app/) (Rust) and React. The Practice Chat engine, speech recognition and the Overlay server run in Rust; see [docs/adr](docs/adr) for the main decisions and [GLOSSARY.md](GLOSSARY.md) for the terms.

Prerequisites: the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (Rust, plus the Xcode command line tools on macOS or the MSVC build tools on Windows), Node.js LTS, and [CMake](https://cmake.org/download/) for building whisper.cpp (`brew install cmake` on macOS; on Windows, tick "C++ CMake tools for Windows" in the Visual Studio Build Tools installer).

```sh
npm install
npm run tauri dev     # run the app
npm run tauri build   # build installers for this OS
cd src-tauri && cargo test
```

## Releasing

For maintainers. GitHub Actions builds the Windows and macOS installers. Push a version tag such as `v0.1.0` and a draft GitHub Release is created with the installers attached. Builds from pull requests and manual runs are attached to the workflow run as artifacts.
