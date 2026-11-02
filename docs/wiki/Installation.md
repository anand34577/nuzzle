## What you need

| Requirement | Details |
|---|---|
| Windows | Windows 10 or 11, 64-bit. There is no macOS or Linux build yet. |
| Graphics card | Any GPU with Vulkan support (recent NVIDIA, AMD and Intel cards all have it). 4 GB of video memory or more is comfortable. Without one, set GPU layers to 0 and the model runs on the processor, more slowly. |
| Disk space | About 1.4 GB for the model, plus the installer. |
| Microphone | Optional, needed only for voice. |

Keep your graphics driver current. Vulkan problems are almost always an old driver.

## Install from a release

1. Open the [Releases page](https://github.com/anand34577/companion/releases) and download `Companion_<version>_x64-setup.exe`. An `.msi` is there too if you prefer one.
2. Optionally check the file against `SHA256SUMS.txt` from the same release:

   ```powershell
   Get-FileHash .\Companion_0.1.0_x64-setup.exe -Algorithm SHA256
   ```

   The hash printed should match the line for that file.
3. Run the installer. Windows may show a SmartScreen warning because the installer isn't code-signed yet. Choose *More info*, then *Run anyway*.
4. Companion starts when the install finishes. Look for a small icon in the system tray (bottom right, possibly under the `^` arrow).

llama.cpp is part of the installer, so there is nothing else to install.

## Build from source

You need Node.js 22, the stable Rust toolchain and PowerShell.

```powershell
npm install
npm run fetch:llama
npm run build
```

`npm run fetch:llama` downloads the pinned llama.cpp release and refuses it if the SHA-256 doesn't match. The installers end up in `src-tauri\target\release\bundle\nsis` and `...\msi`.

Next: [Getting started](Getting-Started).
