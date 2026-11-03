# Downloads the pinned llama.cpp Windows Vulkan release into src-tauri/resources/llama so the app can bundle it.
#   Version : llama.cpp.version  (release tag, e.g. b12038)
#   Checksum: llama.cpp.sha256   (SHA-256 of the release zip; the download is rejected if it differs)
# Usage: npm run fetch:llama      (add -Force to re-download)
param([switch]$Force)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$tag = (Get-Content (Join-Path $root 'llama.cpp.version') -Raw).Trim()
$expected = (Get-Content (Join-Path $root 'llama.cpp.sha256') -Raw).Trim().ToLower()
$dest = Join-Path $root 'src-tauri/resources/llama'
$marker = Join-Path $dest 'VERSION'

if ($tag -notmatch '^b\d+$') { throw "llama.cpp.version must look like b12038, got '$tag'" }
if ($expected -notmatch '^[0-9a-f]{64}$') { throw 'llama.cpp.sha256 must be a 64-character hex SHA-256' }

if (-not $Force -and (Test-Path $marker) -and ((Get-Content $marker -Raw).Trim() -eq $tag) -and (Test-Path (Join-Path $dest 'llama-server.exe'))) {
  Write-Host "llama.cpp $tag already present in $dest"
  exit 0
}

$asset = "llama-$tag-bin-win-vulkan-x64.zip"
$url = "https://github.com/ggml-org/llama.cpp/releases/download/$tag/$asset"
$tmp = Join-Path ([IO.Path]::GetTempPath()) "nuzzle-llama-$tag"
New-Item -ItemType Directory -Force $tmp | Out-Null
$zip = Join-Path $tmp $asset

Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
$actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected) {
  Remove-Item $zip -Force
  throw "SHA-256 mismatch for $asset`n expected $expected`n actual   $actual"
}
Write-Host "Checksum OK ($actual)"

# Replace previous binaries but keep README.txt
Get-ChildItem $dest -Exclude 'README.txt' -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force
Expand-Archive -Path $zip -DestinationPath $dest -Force
# Some releases nest everything in a folder; flatten so llama-server.exe sits directly in $dest.
if (-not (Test-Path (Join-Path $dest 'llama-server.exe'))) {
  $found = Get-ChildItem $dest -Recurse -Filter 'llama-server.exe' | Select-Object -First 1
  if (-not $found) { throw 'llama-server.exe not found in the downloaded archive' }
  Get-ChildItem $found.Directory.FullName | Move-Item -Destination $dest -Force
}
Set-Content -Path $marker -Value $tag -NoNewline
Remove-Item $tmp -Recurse -Force
Write-Host "llama.cpp $tag ready in $dest"
