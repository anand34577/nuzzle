Settings are saved as `settings.json` in `%APPDATA%\dev.companion.desktop`. Tokens for connectors and MCP servers are not in that file; they're in Windows Credential Manager. Everything below is edited in the Settings window.

## Setup

| Setting | What it does |
|---|---|
| Inference engine | Which `llama-server` to use. The default is the bundled one. You can point it at your own build. |
| Model | The LFM3-Omni files. Download them here, or choose files you already have. |
| Graphics cards | Which GPU(s) run the model. |

## Character, Chat, Memory

| Tab | What it does |
|---|---|
| Character | Choose a character, size, playfulness (quiet, normal, lively) and the roaming area. |
| Chat | Shows this session's conversation. It's kept in memory only and is gone when the app quits. |
| Memory | Facts the character remembers, and reminders. You can delete either. |

## Tools

Built-in tools, MCP servers and web connectors. See [Using Nuzzle](Usage#mcp-servers).

## Voice & sight

| Setting | What it does |
|---|---|
| Microphone, speaker | Choose devices. A level meter, chime and voice test are built in. If a headset is unplugged, Nuzzle switches device and tells you. |
| Volume, mute spoken replies | Mute keeps the text bubbles. |
| Language you speak | Helps it understand you. |
| Push to talk | Hold `Ctrl+Shift+Space` to speak. |
| My voice | Try other voices, or give it a recording of a voice you have permission to use. |

## Safety

| Mode | Behaviour |
|---|---|
| Ask for risky things (default) | Asks before writing files, running commands, reading your clipboard and pressing Enter. |
| Ask for everything | Asks before every action. |
| Full access | Never asks. A warning appears once before you can turn it on. |

## Appearance and Advanced

| Setting | What it does |
|---|---|
| Theme, density, font | Light, dark or system; compact or comfortable. |
| GPU layers | 99 puts the whole model on the GPU. 0 uses the processor only. |
| Memory size (tokens) | The context window. Larger values remember more of the conversation and use more GPU memory. |
| Extra llama-server options | Passed straight to `llama-server`. Leave empty unless you know you need something. |
| Open log folder | Opens the folder with the app's log files. |
