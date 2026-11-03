# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
