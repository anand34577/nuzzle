# Companion

A desktop companion that lives on your screen and talks to a local model through llama.cpp (Vulkan). It runs from the
system tray: closing or hiding the character keeps it running; right-click the tray icon to quit.

## Run (development)

```
npm install
npm run fetch:llama     # downloads the pinned llama.cpp (Vulkan) into src-tauri/resources/llama, checksum-verified
npm run dev
```

First launch opens **Settings** with a short setup: llama.cpp is already built in, so you only pick (or one-click
download) the model, choose your graphics card(s) and press *Save & wake me up*.

## llama.cpp is bundled and pinned

| File | Purpose |
|---|---|
| `llama.cpp.version` | The exact llama.cpp release tag that gets bundled (currently `b12038`). |
| `llama.cpp.sha256` | SHA-256 of that release's `llama-<tag>-bin-win-vulkan-x64.zip`. The download is rejected if it differs. |
| `scripts/fetch-llama.ps1` | Downloads, verifies and unpacks it into `src-tauri/resources/llama`. |
| `.github/workflows/build.yml` | CI: clippy + tests, then fetches that same pinned version and builds the installer with it bundled. |

The app uses, in order: the llama-server you chose in Settings -> the bundled one -> `llama-server` on PATH.

**Updating llama.cpp:** change `llama.cpp.version` and `llama.cpp.sha256` together in one commit (the checksum is shown
as the release asset's `digest` on GitHub), run `npm run fetch:llama -- -Force`, and test before merging. LFM3-Omni needs
a build at or after llama.cpp PR #31257 (`b12038` includes it).

## What I can do

- **Talk and listen** (voice) or **type** (`Ctrl+Alt+C`, or double-click the character). Settings -> Voice & sight lets you
  pick the microphone and speaker (with a live level meter, chime and voice test), set the volume, mute spoken replies,
  choose the language you speak, and switch to **push to talk** (hold `Ctrl+Shift+Space`; pressing it while I talk
  interrupts me). Unplug a headset and I switch to another device and tell you.
- **Hearing and speaking use LFM3-Omni's own abilities.** I hear your actual voice (tone, mood, language), not just a
  transcript; I speak as the first audio is generated (streaming); each character has a stable voice of their own (made
  once and kept in `voices/`), and you can try another voice or give me a recording of a voice you have permission to use.
  Every reply starts with a hidden mood ([happy], [curious], ...) that changes my expression.
- **Voice activity detection** (high-pass filter, adaptive noise floor, hysteresis, hiss rejection) with a visible state:
  a small badge shows when I'm listening and when I hear you speak.
- **Manners:** I stay perfectly still while thinking, talking or being typed to; I keep off your text caret, the spot
  you last clicked and your pointer when it rests on me; and *Playfulness* (quiet / normal / lively) sets how jokey I am.
- **Chat history** for the current session (Settings -> Chat). It is kept in memory only.
- **Show what I'm doing:** a thought bubble while thinking (with the current step, e.g. "Looking at your screen"), and
  the reply streams into a speech bubble as I write it.
- **Built-in tools:** screen view, memory, reminders and timers, date/time, system info, read web pages. With
  *computer control* on: mouse and keyboard, read/list/write files, run commands, clipboard, open links.
- **MCP servers:** add your own in Settings -> Tools (local program over stdio, or an HTTP endpoint). Their tools appear
  to the model as `mcp_<server>_<tool>`.
- **12 original characters** (Pip, Mochi, Bruno, Zap, Nimbus, Pebble, Ember, Bun, Axo, Wisp, Penny, Cosmo), each with its
  own temperament, idle habits and reactions. Eyes follow your pointer; poke them, pick them up, let them sleep.
- **Roaming area:** draw a rectangle on your screen (Settings -> Character) to keep me inside it; otherwise I roam anywhere.

## Safety

- Computer control starts **off** every launch. Risky actions need an on-screen *Allow*.
- Settings -> Safety: *Ask for risky things* (default; includes pressing Enter, which may send something), *Ask for
  everything*, or *Full access* (never ask; a warning is shown once before it can be enabled). The approval card shows
  a countdown; no answer counts as no.
- Web pages I read are checked at the address they really resolve to (redirects included), so a link can't point me at
  this PC or your network unless full access is on.
- Tokens for connectors and MCP servers are stored in Windows Credential Manager, not in `settings.json`.
- The model download is pinned to a fixed revision and its SHA-256 is verified.
- `Ctrl+Alt+Space` stops everything instantly. The model server listens on a random local port with a per-launch key.

## Tests

```
cd src-tauri && cargo test && cargo clippy --all-targets
```
