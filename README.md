<picture>
  <img alt="Nuzzle: a small character on your desktop that talks, listens and helps" src="assets/banner.png" width="100%">
</picture>

[![Build](https://github.com/anand34577/nuzzle/actions/workflows/build.yml/badge.svg)](https://github.com/anand34577/nuzzle/actions/workflows/build.yml)
![Version](https://img.shields.io/badge/version-0.4.0-5a4bd1)
![License](https://img.shields.io/badge/license-Apache--2.0-5a4bd1)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-444)

Nuzzle is a desktop app for Windows, macOS and Linux. A small animated character lives on your screen, listens when you talk, can look at
what you're doing, and answers out loud. The language model runs on your own graphics card through llama.cpp, so nothing
you say or show leaves the machine.

<p align="center"><img alt="Pip, the default character, pointing out a bug in a Python file and offering to fix it" src="assets/screenshot-chat.png" width="860"></p>

A [11-second clip](assets/nuzzle-promo.mp4) (1080×1080 MP4) is included for sharing on X and LinkedIn.

## Why

Most assistants live in a browser tab and only know what you paste into them. Nuzzle sits next to your work, sees the
screen when you let it, and can act on it with your approval. The model is small enough (about 1.4 GB) to run on a
mid-range GPU, and everything stays local.

## Features

- Talks and listens. It hears your voice directly (tone and language, not just a transcript) and starts speaking before
  the full reply is written.
- Sees your screen when you turn that on, and shows what it's doing in a thought bubble ("Looking at your screen").
- Controls the mouse, keyboard, files and shell, only when computer control is on, and asks before anything risky.
- Remembers facts you tell it, sets reminders and timers, searches the web, checks the weather, keeps notes and drafts
  emails for you to send.
- Quick commands that need no model at all: `/timer 10m tea`, `/remind every 1h drink water`, `/note buy milk`,
  `/calc`, `/convert 5 km to miles`, `/roll 2d6`. Type `/help` to see them. They work while the model is still
  downloading or loading, and from Telegram and the agent API too.
- Bring your own brain and voice: think with any OpenAI-compatible chat service instead of the local model, and hear
  and speak through your own speech servers (Settings -> Advanced). `scripts/speech-server.py` runs Hima-ASR and
  Paradee locally for that. With a service on the internet, what you say, type and show is sent to it.
- Acts like a little pet: right-click it to give a treat, play or send it for a nap; drop it and it falls; call it and it
  runs over. It gets hungry, lonely and bored if ignored (gently, never annoyingly).
- Chat with it from your phone over Telegram, and let other agents (OpenClaw, PicoClaw, scripts) talk to it through a
  local API.
- Connects to your own MCP servers: pick one from a gallery, type one command line, or paste an `mcpServers` JSON.
- Comes with 12 original characters, each with its own voice, habits and reactions.
- Keeps secrets in the system credential store (Windows Credential Manager, macOS Keychain, the Linux keyring), not in the settings file.

<p align="center"><img alt="Approval card asking permission before editing a file" src="assets/screenshot-approval.png" width="620"></p>

## Quick start

1. Download the installer for your system from the [Releases](https://github.com/anand34577/nuzzle/releases) page:
   `*_x64-setup.exe` or `.msi` (Windows), `*_aarch64.dmg` (Apple silicon) or `*_x64.dmg` (Intel Mac), `.deb`, `.rpm` or
   `.AppImage` for Linux on x64 or ARM64.
2. Open **Settings** from the tray icon. On first launch it opens by itself.
3. In **Setup**, press *Download model* (about 1.4 GB, resumable, checksum-verified), pick your graphics card, then
   press *Save & wake me up*.

llama.cpp is already inside the installer. You should see your character appear and say hello. The
[Getting started](https://github.com/anand34577/nuzzle/wiki/Getting-Started) wiki page walks through each step.

## Build from source

You need Node.js 22 and the stable Rust toolchain. On Linux, also install the libraries Tauri, audio and screen
capture build against (Debian/Ubuntu names; the full list is `LINUX_DEPS` in the [build workflow](.github/workflows/build.yml)):

```bash
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev libxdo-dev libdbus-1-dev libxcb1-dev libxrandr-dev libpipewire-0.3-dev
```

Then on any system:

```bash
npm install
npm run fetch:llama     # downloads the pinned llama.cpp for this OS into src-tauri/resources/llama, checksum-verified
npm run dev
```

`npm run build` produces the installers under `src-tauri/target/release/bundle/`: NSIS and MSI on Windows, a `.dmg`
on macOS, and `.deb`, `.rpm` and AppImage on Linux.

### Platform notes

- **macOS**: grant Microphone, Screen Recording and Accessibility when asked (Settings -> Safety has buttons for each
  pane). Shortcuts use Cmd where Windows uses Ctrl, so stop is Cmd+Option+Space. The app isn't notarized yet; right-click
  it and choose *Open* the first time.
- **Linux**: the packages need glibc 2.39 or newer (Ubuntu 24.04+, Debian 13, Fedora 40+). X11 works best. Wayland desktops restrict global shortcuts, screen capture, pointer control and
  click-through windows. Clipboard tools need `wl-clipboard` or `xclip`; saved tokens need a running keyring
  (GNOME Keyring or KWallet).

## Releases

Pushing a tag that starts with `v` runs the [build workflow](.github/workflows/build.yml): clippy and tests, then a
Windows, macOS and Linux build, then a GitHub Release with every installer and a `SHA256SUMS.txt` attached.

```bash
git tag v0.1.1 && git push origin v0.1.1
```

Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` first, and add a line to
[CHANGELOG.md](CHANGELOG.md).

## llama.cpp is bundled and pinned

| File | Purpose |
|---|---|
| `llama.cpp.version` | The exact llama.cpp release tag that gets bundled (currently `b12038`). |
| `llama.cpp.sha256` | SHA-256 of that release's `llama-<tag>-bin-win-vulkan-x64.zip`. The download is rejected if it differs. |
| `llama.cpp.sha256.<platform>` | The same for `linux-x64`, `linux-arm64` (Vulkan) and `macos-arm64`, `macos-x64` (Metal). |
| `scripts/fetch-llama.mjs` | Downloads, verifies and unpacks the right build for this OS into `src-tauri/resources/llama`. |
| `.github/workflows/build.yml` | CI: clippy and tests, then fetches that same pinned version and builds the installer with it bundled. |

The app uses, in order: the llama-server you chose in Settings, the bundled one, then `llama-server` on PATH.

**Updating llama.cpp:** change `llama.cpp.version` and every `llama.cpp.sha256*` file together in one commit (each
checksum is shown as the release asset's `digest` on GitHub), run `npm run fetch:llama -- --force`, and test before
merging. LFM3-Omni needs
a build at or after llama.cpp PR #31257 (`b12038` includes it).

## What it can do

- **Talk and listen** (voice) or **type** (`Ctrl+Alt+C`, or double-click the character). Settings -> Voice & sight lets you
  pick the microphone and speaker (with a live level meter, chime and voice test), set the volume, mute spoken replies,
  choose the language you speak, and switch to **push to talk** (hold `Ctrl+Shift+Space`; pressing it while I talk
  interrupts me). Unplug a headset and I switch to another device and tell you.
- **Hearing and speaking use LFM3-Omni's own abilities.** I hear your actual voice (tone, mood, language), not just a
  transcript; I speak as the first audio is generated (streaming); each character has a stable voice of their own (made
  once and kept in `voices/`), and you can try another voice or give me a recording of a voice you have permission to use.
  Every reply starts with a hidden mood ([happy], [curious], ...) that changes my expression.
- **Voice activity detection** (high-pass filter, adaptive noise floor, hysteresis, hiss rejection, and a steady noise
  such as a fan is learned instead of transcribed) with a visible state. The moment I hear you I stop walking, turn to
  you, lean in and cup a hand to my ear; a half-heard sentence is dropped if the mic closes, never glued to the next.
- **Manners:** I stay perfectly still while thinking, talking or being typed to; I keep off your text caret, the spot
  you last clicked and your pointer when it rests on me; and *Playfulness* (quiet / normal / lively) sets how jokey I am.
- **Chat history** for the current session (Settings -> Chat). It is kept in memory only.
- **Show what I'm doing:** a thought bubble while thinking (with the current step, e.g. "Looking at your screen"), and
  the reply streams into a speech bubble as I write it.
- **Built-in tools:** screen view, memory, reminders and timers, date/time, an exact calculator, system info, the
  list of open windows, web search and reading web pages. With *computer control* on: mouse and keyboard, find, read,
  list and write files, open files, folders and apps, run commands, clipboard, open links.
- **MCP servers:** Settings -> Tools. Click a ready-made one (Files, Knowledge graph, Web fetch, Time zones, Git, GitHub,
  Browser), type a single command line such as `npx -y @modelcontextprotocol/server-memory` or an `https://` address, or
  paste the `mcpServers` JSON from Claude Desktop, Cursor or any MCP guide. Keys go in the small *Keys* box. Their tools
  appear to the model as `mcp_<server>_<tool>`.
- **12 original characters** (Pip, Mochi, Bruno, Zap, Nimbus, Pebble, Ember, Bun, Axo, Wisp, Penny, Cosmo), each with its
  own temperament, idle habits and reactions. Eyes follow your pointer; poke them, pet them, pick them up, let them sleep.
- **A pet, not a widget:** right-click me for *Give a treat* (each character has a favourite), *Play / do a trick*
  (dance, backflip, bow), *Come here* and *Take a nap*, with little meters for tummy, love, fun and energy. Drop me in
  mid-air and I fall onto the window below or the ground; I get the zoomies now and then; I yawn and nap sooner when
  tired, and ask for a treat or a pat when I've been ignored (never in *quiet* mode).
- **Helpful tools:** `email_draft` opens a finished email in your mail app (you press Send), `notes` keeps Markdown notes
  in *Documents/Nuzzle Notes*, `weather` gives the forecast (Open-Meteo, no account), `media_key` plays, pauses and
  changes volume, and `come_here` brings me to your pointer.
- **Telegram** (Settings -> Remote): make a bot with @BotFather, paste its token, press *Pair my phone* and send the code
  to your bot. Only paired chats are answered; risky actions show *Allow / No* buttons in the chat; `/stop` stops me.
- **Agent API** (Settings -> Remote): `POST /message {"text": "..."}` or OpenAI-style `POST /v1/chat/completions`, with
  a bearer token, on `127.0.0.1:7457` (or your network if you allow it). Lets OpenClaw, PicoClaw, n8n or a script ask me
  things. (Telegram bots can't message each other, so agents use this instead.)
- **Roaming area:** draw a rectangle on your screen (Settings -> Character) to keep me inside it; otherwise I roam anywhere.

<p align="center"><img alt="The twelve built-in characters" src="assets/screenshot-characters.png" width="720"></p>

## Safety

- Computer control starts **off** every launch. Risky actions need an on-screen *Allow*.
- Settings -> Safety: *Ask for risky things* (default; includes pressing Enter, which may send something), *Ask for
  everything*, or *Full access* (never ask; a warning is shown once before it can be enabled). The approval card shows
  a countdown; no answer counts as no.
- Web pages I read are checked at the address they really resolve to (redirects included), so a link can't point me at
  this PC or your network unless full access is on.
- Tokens for connectors and MCP servers are stored in the system credential store, not in `settings.json`.
- The model download is pinned to a fixed revision and its SHA-256 is verified.
- `Ctrl+Alt+Space` stops everything instantly. The model server listens on a random local port with a per-launch key.
- llama-server and MCP servers can't outlive the app, even after a crash: a Job Object on Windows, and on macOS/Linux
  their own process group watched by a tiny watchdog that cleans up if Nuzzle disappears.
- Telegram answers only chats paired with a one-time code; the agent API needs its token and listens only on this
  computer unless you allow your network. Both tokens live in the credential store.

## Tests

```powershell
cd src-tauri
cargo test
cargo clippy --all-targets
```

## More

The [wiki](https://github.com/anand34577/nuzzle/wiki) covers installation, every setting, troubleshooting and how
the app is put together. The same pages live in [docs/wiki](docs/wiki).

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Nuzzle is released under the [Apache License 2.0](LICENSE). You can use, modify and redistribute it, including in
commercial work, as long as you keep the license and [NOTICE](NOTICE) and state your changes. The software comes with no
warranty. The bundled llama.cpp is MIT-licensed, and the LFM3-Omni model that the app downloads is under LiquidAI's own
license (see [NOTICE](NOTICE)).
