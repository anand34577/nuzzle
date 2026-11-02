Companion is a [Tauri 2](https://tauri.app) app: a Rust backend in `src-tauri/src` and a plain HTML, CSS and JavaScript front end in `src` (no bundler).

## Layout

| Path | Role |
|---|---|
| `src/index.html`, `main.js`, `style.css` | The character window: bubbles, approval card, toolbar. |
| `src/characters.js` | The 12 characters as SVG on one shared rig, plus their lines. |
| `src/settings.*` | The Settings window. |
| `src/area.*` | Full-screen overlay for drawing the roaming area. |
| `src-tauri/src/brain.rs` | Talks to the local model and runs the tool loop. |
| `src-tauri/src/tools.rs`, `control.rs`, `screen.rs` | Built-in tools, input control, screen capture. |
| `src-tauri/src/voice.rs`, `audio.rs`, `vad.rs` | Microphone, speech output, voice activity detection. |
| `src-tauri/src/mcp.rs`, `secrets.rs` | MCP clients and Credential Manager storage. |
| `src-tauri/src/download.rs`, `setup.rs` | Pinned, checksum-verified model download and first-run setup. |
| `src-tauri/src/memory.rs`, `reminders.rs`, `settings.rs` | Persistent data. |
| `src-tauri/resources/llama` | Bundled llama.cpp, filled by `scripts/fetch-llama.ps1` and not committed. |

## How a reply happens

1. Audio (and a screenshot if screen viewing is on) goes to a local `llama-server` process started on a random port with a per-launch key.
2. The model streams text. The front end shows it in the speech bubble as it arrives, and speech starts once the first audio is generated.
3. If the model calls a tool, `tools.rs` decides whether it needs approval, shows the card, runs it, and feeds the result back.

## CI and releases

`.github/workflows/build.yml` runs clippy and tests, builds the installers with the pinned llama.cpp and uploads them as an artifact. On a `v*` tag it also creates a GitHub Release with the installers and checksums.
