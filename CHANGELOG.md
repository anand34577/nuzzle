# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.4.0] - 2026-11-11

### Added
- Quick commands: a message starting with a slash is done at once, without the model, so the app is useful before a model is downloaded and while it loads. `/timer`, `/remind` (also `every 1h ...` and `every day at 9am ...`), `/reminders`, `/cancel`, `/note`, `/notes`, `/calc`, `/convert`, `/flip`, `/roll`, `/pick`, `/remember`, `/time`, `/new`, `/stop`, `/help`. They work in the typing box, Settings -> Chat, Telegram and the agent API.
- Repeating reminders ("remind me to drink water every hour"). Settings -> Reminders shows how often each repeats.
- Tools for the model: `convert_units` (length, weight, volume, time, data size, speed, area, temperature) and `random` (coin, dice, number, pick one).
- Right-click menu: *Quick commands*. The focus timer there and in *Try asking me* is now instant and no longer depends on the model.

- Other brains: Settings -> Advanced -> *Brain* can use any OpenAI-compatible chat service (OpenAI, OpenRouter, Groq, Together, Mistral, DeepSeek, or a local Ollama / LM Studio server) instead of the downloaded model. The key is kept in the system credential store. With such a service no local model is needed at all.
- Your own speech engines: Settings -> Advanced -> *Ears and voice* can send hearing and speaking to any server with OpenAI-style `/audio/transcriptions` and `/audio/speech` endpoints, so a text-only model can still listen and talk.
- `scripts/speech-server.py`: a small local server that runs Hima-ASR (speech to text, 11 Indian languages and English) and Paradee (text to speech) behind those endpoints, on the CPU.
- Nuzzle can run that speech server itself: Settings -> Advanced -> *Run the speech server for me* starts it with the app, restarts it when its settings change, stops it on quit and shows whether it is up (and why not).
- Feeling your voice: with the whole Hima repo in the model folder (`hima_onnx.py` and `decide/`), the speech server also reports tone, emotion and urgency. While you are still talking the character's face follows it (asked about every 1.2 s), and the reply is told how you sounded so it can be gentler or livelier.
- A voice of their own: on the built-in speech server each character speaks with their own Paradee voice and pace (Zap quick, Pebble slow), in English, Hindi, French or Spanish following the speech language. A voice typed in Settings is used as it is.
- Morning hello: the first time I see you each morning I say today's reminders and the weather for your town (Settings -> Character). Once a day, only when there is something to say, never in quiet mode. `/today` gives the same any time and `/weather [place]` the forecast.
- The log now times the voice pipeline (`pipeline: heard ... in N ms`, `first word after N ms`, `speech ... started after N ms`).

### Changed
- The local model reads its instructions and tool list as soon as it wakes up (and again when computer control is switched on or off), so the first answer no longer waits for that. Chat requests always use the same server slot, so its cache is reused from turn to turn.
- Pointer moves take about half as long, and the pause after each action is 350 ms instead of 600 ms.

- *Clear* in Settings -> Chat starts a fresh conversation: the model forgets the chat too, not only the list on screen.
- The typing box says what to do when there is no model yet, instead of "I'm still waking up".
- `set_reminder` accepts numbers sent as text and durations such as "in 2 hours", which small models often produce.

### Fixed
- Reading web pages: any page with a `<header>` element lost everything after it (the `<head>` remover matched it), so most modern sites came back nearly empty.
- Reading web pages: a page containing certain non-English letters before a script or style block could crash the reply. Numeric entities such as `&#39;` are now decoded.
- A reminder that came due while the app was closed was dropped silently at the next launch (it fired before the character was on screen). It now waits and says it was due earlier.
- Stopping me mid-reply left an unanswered question in the conversation, which could confuse the next reply.
- The right-click menu showed `Ctrl+Alt+C` even on a Mac or after the shortcut was changed, and could be cut off when the character is small.
- Telegram pairing: a code is thrown away after 5 wrong guesses, so a six-digit code can't be brute-forced.
- Sites whose names start with `fc`, `fd` or `fe80` (for example fcbarcelona.com) were wrongly treated as local-network addresses and blocked.

## [0.3.0] - 2026-11-03

### Changed
- The app is now called **Nuzzle** (it was "Companion"), with a new fox-face icon. Your settings, memory and saved tokens carry over; an existing "Companion Notes" folder keeps being used.
- A calmer, friendlier Settings window: indigo and orange palette, grouped navigation, toggles as proper rows, and a save bar that only appears when there is something to save.
- Fixed fields that rendered tiny or unstyled (Telegram token, URLs) and the cramped MCP server rows.

### Added
- Welcome hero with live setup progress and the next step named.
- Quick search (Ctrl+K) to jump to any setting, and Ctrl+S to save.
- "Try asking me" one-tap prompts in Chat (screen summary, focus timer, stretch reminder, weather).
- Loading states everywhere: a moving bar under the title bar while loading or saving, placeholder rows for lists, a spinner on Save and on one-tap prompts, and a progress bar on MCP servers that are starting.
- A redesigned MCP server screen: icon tiles for each server type, a status pill, tool chips, and an error banner with a Try again button.
- Right-click menu on the pet: "What's on my screen?" and a 25 minute focus timer.

## [0.2.0] - 2026-11-03

### Added
- Release builds for every platform: Windows x64 (`.exe`, `.msi`), macOS Apple silicon and Intel (`.dmg`), Linux x64 and ARM64 (`.deb`, `.rpm`, AppImage).
- A pet to play with: right-click menu with treats (each character's favourite), tricks (dance, backflip, bow), *Come here* and naps; tummy / love / fun / energy needs that gently change behaviour; falling with gravity when dropped; zoomies; new idle quirks (scratch, shiver).
- Listening that looks like listening: the character stops, faces you, leans in and cups an ear the moment you start talking.
- Telegram: chat from your phone with a paired bot, approvals as buttons in the chat, `/stop`.
- Agent API: token-protected local `POST /message` and OpenAI-style `/v1/chat/completions` for OpenClaw, PicoClaw and other agents.
- Tools: `email_draft`, `notes`, `weather`, `media_key`, `come_here`.
- Simpler MCP setup: a gallery of ready servers, one command-line field, a *Keys* box, and *Paste config* for `mcpServers` JSON.
- New tools: `calculate` (exact arithmetic), `web_search` (DuckDuckGo, asked once per session), `list_windows`, and with computer control `open_path`, `launch_app` and `find_files`.
- macOS permission shortcuts (Microphone, Screen Recording, Accessibility) and Linux notes in Settings -> Safety.
- The typing box remembers what you sent this session (Up/Down), and clicking a finished speech bubble puts it away.

### Fixed
- llama-server and MCP servers are cleaned up after a crash or `kill -9` on macOS and Linux too (process group plus watchdog), and on quit their whole process tree goes.
- Voice detection: a sentence cut off by the mic closing (or the companion starting to speak) is dropped instead of being glued to the next one; a fan or music that never pauses is learned as background instead of being sent off every 25 seconds; the character no longer wanders off while you are mid-sentence.
- macOS/Linux: times and "remind me at 3pm" used UTC instead of local time.
- macOS/Linux: the clipboard tools ran PowerShell commands and always failed; they now use pbcopy/pbpaste, wl-clipboard or xclip.
- macOS/Linux: saved connector tokens were written to an in-memory store and lost on restart. They now go to the Keychain or the Secret Service keyring, and a store that can't give a value back no longer swallows it.
- macOS/Linux: poking the character, double-click to chat and "stay out of the way while typing" did nothing (no mouse-button or idle detection).
- macOS: shortcuts recorded with Ctrl were saved as Cmd; key hints now show ⌘/⌥, and the tray icon opens its menu on click.
- The model is told which OS it runs on, so commands and key combos fit it. System info reports memory, uptime and the computer name on every OS.
- `clipboard_get` returned the error text as if it were the clipboard when the command failed.

## [0.1.1] - 2026-11-02

### Changed
- Cuter and livelier: shinier eyes, a gentle idle bounce, a happy squish when the pointer comes over the character, dust puffs on landing, star-shaped sparkles, and speech bubbles that pop in.
- Staying out of the way: the character no longer hops to a spot beside the pointer. While the pointer is moving, or you are typing, he picks places to stand well away from it and from your text. He never runs from a pointer that comes to him, so he can still be petted, poked and dragged.

## [0.1.0] - 2026-11-02

### Added
- Animated desktop character with 12 built-in characters, tray icon, roaming area and idle behaviours.
- Voice conversation using LFM3-Omni: direct audio input, streamed speech output, push to talk, voice activity detection.
- Screen viewing, memory, reminders and timers, web page reading.
- Computer control (mouse, keyboard, files, shell, clipboard) with an approval card for risky actions.
- MCP server support over stdio and HTTP, and web connectors.
- Bundled llama.cpp `b12038` (Vulkan), verified against a pinned SHA-256.
- Apache-2.0 license, NOTICE file and contributing guide.
- GitHub Actions workflow that tests, builds the Windows installers and publishes them on `v*` tags.
