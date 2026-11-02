## Privacy

- The model runs locally. Audio, screenshots and conversation text are not sent anywhere.
- The only network traffic is the model download from Hugging Face, web pages you ask it to read, and whatever your own MCP servers or connectors do.
- Chat history lives in memory and is gone when you quit. Memories and reminders are kept in files in the app's data folder, and you can clear them in Settings.
- Tokens are stored in Windows Credential Manager.

## What it asks before doing

Computer control starts **off** every launch. With it on, the default mode asks before risky actions: writing files, running commands, reading the clipboard, opening links and pressing Enter (which may send something). The card shows a countdown, and no answer counts as no. `Enter` allows, `Esc` refuses.

## Built-in protections

- Web pages are checked at the address they really resolve to, redirects included, so a link can't point it at your PC or your network unless full access is on.
- The model download is pinned to a fixed revision and its SHA-256 is verified.
- The local model server listens on a random port and needs a per-launch key.
- `Ctrl+Alt+Space` stops everything.

## Full access

It never asks. Only turn it on for a task you're watching.
