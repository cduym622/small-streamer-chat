# MVP build plan

Each milestone ends with something you can run and show. The order puts the riskiest work first (Windows builds, Whisper on Windows) and gets an end-to-end Practice Chat working as early as possible. Terms follow `GLOSSARY.md`; decisions are in `docs/adr/`.

## M0 — Skeleton that ships
- Tauri v2 + React + TypeScript app at the repo root
- Tray / menu-bar icon; closing the window hides it to the tray (one-time notice); single instance
- GitHub Actions (`tauri-action`) builds unsigned Windows `-setup.exe` and macOS (Apple Silicon + Intel) `.dmg` into a draft Release
- **Done when:** you install the CI-built `.exe` on the Windows PC and the `.dmg` on the Mac, and both open

## M1 — Practice Chat with canned messages
- Rust engine loop posting canned AI Chatter messages on the Q7 timing rules
- Practice Chat window in the app (Rust → webview events)
- Overlay: a localhost server in Rust serving the plain-TS Overlay page, with live messages pushed to it; Twitch-style, transparent, AI Badge
- Global hotkeys: start/stop, Back-off, Nudge
- **Done when:** canned chat scrolls in an OBS browser source on both OSes while the app window is minimized for 10+ minutes

## M2 — Hearing the streamer
- Microphone picker; capture in Rust; convert audio to the format Whisper needs (16 kHz mono)
- Pause detection (doubles as the "after a pause" trigger from Q7)
- Whisper model (multilingual base) downloaded on first launch with progress; transcription via `whisper-rs` (Metal on Mac, CPU on Windows)
- macOS microphone permission prompt
- Debug panel showing the live transcript
- Add `whisper-rs` to CI early in this milestone. Windows build issues are the biggest unknown.
- **Done when:** you speak and see the transcript within ~2 s on both machines. Check CPU use on the Windows PC while a game is running.

## M3 — The loop closes (Anthropic)
- Anthropic API key entry, stored in the OS keychain, plus a test button
- Prompt from: transcript + Stream Context + 3 fixed Personas + "normal teasing" Content Level and always-blocked rules
- Replace canned messages with Haiku output; AI Chatters never reference Real Chatters
- Running cost display
- **Done when:** a full stream session works end to end: talk, and relevant AI messages appear in OBS

## M4 — Sign in with ChatGPT
- "Continue with ChatGPT" OAuth flow: PKCE, system browser, loopback on `127.0.0.1:1455`, token refresh, tokens in the keychain
- Model list from `/v1/models`, preselecting the smallest/fastest, with a dropdown
- Streaming Responses API with `store:false`; plan-allowance errors shown as a notice in the app; cost display shows "using your ChatGPT plan"
- **Done when:** the same session works on a ChatGPT plan and runs out of allowance gracefully

## M5 — Ready for strangers (v0.1.0)
- First-run setup flow: mic → model download → AI Connection → Stream Context → OBS instructions
- Clear errors: no mic permission, download failed, bad key, offline
- README: what it is, a GIF, install steps with screenshots of the SmartScreen / Gatekeeper / Privacy & Security warnings, and the OBS setup
- **Done when:** a non-technical friend installs it from the Release page and uses it without your help

## Deliberately not in the MVP
Persona editor, Content Level setting, Stream Memory, spending cap, OpenAI API key / Gemini / Ollama, Overlay customization, pinned language, cloud speech-to-text fallback, Vulkan on Windows, code signing.
