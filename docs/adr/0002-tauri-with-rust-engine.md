# Tauri, with the whole engine in the Rust core

The app is built with Tauri. Microphone capture, Whisper transcription, message scheduling, the AI provider call, hotkeys and the Overlay server all run in the Rust core. The TypeScript webview only renders the settings screens and the Practice Chat window. Streamers minimize the app for hours while gaming. On Windows, Tauri can't turn off webview background throttling, so timers slow down and a hidden page can be paused after about five minutes ([config reference](https://v2.tauri.app/reference/config/)). Logic in the webview would quietly stop working mid-stream.

## Considered Options

- **Tauri with the engine in webview TypeScript**: rejected because of the Windows throttling problem above.
- **Tauri with whisper.cpp's `whisper-server` as a bundled helper program**: less Rust, but an extra per-OS binary to build and manage. Rejected in favour of linking Whisper directly through `whisper-rs`.
- **Electron with everything in TypeScript**: no throttling problem and no Rust. Rejected for its larger download and memory use on machines already running a game and OBS. The project owner was also happy to take on Rust.
