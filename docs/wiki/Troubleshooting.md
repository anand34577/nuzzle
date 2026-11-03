## The engine step is red

The bundled `llama-server.exe` is missing or blocked. If you built from source, run `npm run fetch:llama` and restart. If you installed from a release, reinstall; antivirus sometimes quarantines it, so add an exclusion for the Nuzzle install folder.

## Windows says "Windows protected your PC"

The installer isn't code-signed. Choose *More info*, then *Run anyway*. You can check the file against `SHA256SUMS.txt` on the release page first.

## The character never wakes up

Open the log folder from Settings and read the newest file. The usual causes:

- **Out of graphics memory.** Lower *Memory size (tokens)* in Advanced, or set GPU layers to a smaller number.
- **Old graphics driver.** Update it, since Vulkan support depends on the driver.
- **Wrong card selected.** Pick the dedicated GPU in Setup, step 3.

## The model download stops

Press *Download model* again. It resumes where it left off. If a checksum fails, the file is rejected and fetched again, so make sure the disk isn't full.

## It can't hear me

Pick the right microphone in *Voice & sight* and watch the level meter while you speak. Also check that Windows Settings > Privacy > Microphone allows desktop apps.

## "Ctrl+Alt+Space is taken by another app"

Another program owns that shortcut. Use the stop button on the toolbar or the tray menu to stop the character.

## It keeps moving onto what I'm typing

It avoids your text caret, the last place you clicked, and your pointer when it rests on it. If it still gets in the way, draw a smaller roaming area in *Character*.
