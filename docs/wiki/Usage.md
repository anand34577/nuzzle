## Talking and typing

- **Voice:** speak while the microphone button on the toolbar is on. A small badge shows when it's listening and when it hears you.
- **Push to talk:** turn it on in *Voice & sight*, then hold `Ctrl+Shift+Space` while you speak. Pressing it while the character is talking interrupts it.
- **Typing:** `Ctrl+Alt+C`, or double-click the character.
- **Stop everything:** `Ctrl+Alt+Space` stops speech, tools and any running action at once.

## The toolbar

Hover over the character to show it. From left to right: microphone, screen viewing, computer control, type a message, stop, settings, hide to tray. Computer control is off every time the app starts.

## What it can do for you

| Tool | Needs computer control? | Example |
|---|---|---|
| Look at the screen | No (needs screen viewing) | "What does this error mean?" |
| Memory | No | "Remember that my invoices go out on the 28th." |
| Reminders and timers | No | "Remind me to stretch in 20 minutes." |
| Date, time, system info | No | "How much memory is free?" |
| Read web pages | No | "Summarise this link." |
| Mouse, keyboard, scrolling | Yes | "Click the Export button." |
| Read, list and write files | Yes | "Open ledger.py and look at line 42." |
| Run commands | Yes | "Run the tests." |
| Clipboard, open links | Yes | "Copy that to the clipboard." |

Risky actions show an approval card. It counts down, and no answer counts as no.

![Approval card asking to edit a file](https://raw.githubusercontent.com/anand34577/companion/main/assets/screenshot-approval.png)

## Characters

Pick one in Settings, *Character*. Each has its own voice, idle habits and replies when you poke it. You can also draw a rectangle on screen to keep the character inside it.

![The twelve characters](https://raw.githubusercontent.com/anand34577/companion/main/assets/screenshot-characters.png)

## MCP servers

In *Tools*, add a server either as a local program (command and arguments, talking over stdio) or as an HTTP endpoint. Its tools show up to the model as `mcp_<server>_<tool>`. Tokens you enter are stored in Windows Credential Manager.
