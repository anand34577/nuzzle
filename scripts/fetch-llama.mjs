// Downloads the pinned llama.cpp release for this platform into src-tauri/resources/llama so the app can bundle it.
//   Version : llama.cpp.version            (release tag, e.g. b12038)
//   Checksum: llama.cpp.sha256             (Windows x64 zip; required)
//             llama.cpp.sha256.<platform>  (linux-x64, linux-arm64, macos-arm64, macos-x64)
// When a platform has no pinned checksum file, the SHA-256 GitHub publishes for the release asset is used instead
// and a warning is printed. Pin it (copy the printed value into the file) for reproducible builds.
// Usage: npm run fetch:llama            (add -- --force to re-download)
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const force = process.argv.slice(2).some((a) => /^-{1,2}force$/i.test(a));
const tag = fs.readFileSync(path.join(root, 'llama.cpp.version'), 'utf8').trim();
const dest = path.join(root, 'src-tauri', 'resources', 'llama');
const marker = path.join(dest, 'VERSION');
const win = process.platform === 'win32';
const exe = win ? 'llama-server.exe' : 'llama-server';

const fail = (msg) => { console.error(`fetch-llama: ${msg}`); process.exit(1); };
if (!/^b\d+$/.test(tag)) fail(`llama.cpp.version must look like b12038, got '${tag}'`);

// Which release asset fits this machine. Vulkan builds on Windows and Linux; Metal is built in on macOS.
// LLAMA_ARCH=x64|arm64 fetches for another CPU (cross-building, e.g. an Intel Mac build on Apple silicon).
const arch = process.env.LLAMA_ARCH || (process.arch === 'arm64' ? 'arm64' : 'x64');
const platform = { win32: 'windows', linux: 'linux', darwin: 'macos' }[process.platform];
if (!platform) fail(`unsupported platform ${process.platform}`);
const key = `${platform}-${arch}`;
const pattern = {
  'windows-x64': `llama-${tag}-bin-win-vulkan-x64.zip`,
  'linux-x64': `bin-ubuntu-vulkan-x64`,
  'linux-arm64': `bin-ubuntu-vulkan-arm64`,
  'macos-arm64': `bin-macos-arm64`,
  'macos-x64': `bin-macos-x64`,
}[key];
if (!pattern) fail(`no llama.cpp build is set up for ${key}; install llama-server yourself and pick it in Settings`);

if (!force && fs.existsSync(marker) && fs.readFileSync(marker, 'utf8').trim() === tag && fs.existsSync(path.join(dest, exe))) {
  console.log(`llama.cpp ${tag} already present in ${dest}`);
  process.exit(0);
}

const pinFile = path.join(root, key === 'windows-x64' ? 'llama.cpp.sha256' : `llama.cpp.sha256.${key}`);
let expected = fs.existsSync(pinFile) ? fs.readFileSync(pinFile, 'utf8').trim().toLowerCase() : '';
if (expected && !/^[0-9a-f]{64}$/.test(expected)) fail(`${path.basename(pinFile)} must be a 64-character hex SHA-256`);

const headers = { 'User-Agent': 'companion-fetch-llama', Accept: 'application/vnd.github+json' };
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
const rel = await fetch(`https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/${tag}`, { headers });
if (!rel.ok) fail(`couldn't read release ${tag} from GitHub (HTTP ${rel.status})`);
const assets = (await rel.json()).assets || [];
const asset = assets.find((a) => (a.name === pattern || a.name.includes(pattern)) && /\.(zip|tar\.gz)$/.test(a.name));
if (!asset) fail(`release ${tag} has no asset matching '${pattern}'`);
if (!expected) {
  expected = (asset.digest || '').replace(/^sha256:/, '').toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(expected)) fail(`no pinned checksum in ${path.basename(pinFile)} and GitHub gave none for ${asset.name}`);
  console.warn(`warning: ${path.basename(pinFile)} not found; using GitHub's published SHA-256 ${expected}`);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `companion-llama-${tag}-`));
const archive = path.join(tmp, asset.name);
console.log(`Downloading ${asset.browser_download_url}`);
const res = await fetch(asset.browser_download_url, { headers: { 'User-Agent': headers['User-Agent'] } });
if (!res.ok) fail(`download failed (HTTP ${res.status})`);
const bytes = Buffer.from(await res.arrayBuffer());
const actual = createHash('sha256').update(bytes).digest('hex');
if (actual !== expected) fail(`SHA-256 mismatch for ${asset.name}\n expected ${expected}\n actual   ${actual}`);
fs.writeFileSync(archive, bytes);
console.log(`Checksum OK (${actual})`);

const out = path.join(tmp, 'x');
fs.mkdirSync(out);
const run = (cmd, args) => {
  const r = spawnSync(cmd, args, { stdio: 'inherit' });
  if (r.status !== 0) fail(`${cmd} failed while unpacking ${asset.name}`);
};
if (win) run('powershell', ['-NoProfile', '-Command', `Expand-Archive -LiteralPath '${archive}' -DestinationPath '${out}' -Force`]);
else if (asset.name.endsWith('.zip')) run('unzip', ['-q', archive, '-d', out]);
else run('tar', ['-xzf', archive, '-C', out]);

// Releases sometimes nest everything (build/bin/...); take the folder that holds llama-server, plus licenses.
const find = (dir, name) => {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isFile() && e.name === name) return p;
    if (e.isDirectory()) { const f = find(p, name); if (f) return f; }
  }
  return null;
};
const server = find(out, exe);
if (!server) fail(`${exe} not found in ${asset.name}`);
const binDir = path.dirname(server);

fs.mkdirSync(dest, { recursive: true });
for (const e of fs.readdirSync(dest)) if (e !== 'README.txt') fs.rmSync(path.join(dest, e), { recursive: true, force: true });
for (const e of fs.readdirSync(binDir, { withFileTypes: true })) {
  if (e.isFile() || e.isSymbolicLink()) fs.cpSync(path.join(binDir, e.name), path.join(dest, e.name), { dereference: true });
}
for (const e of fs.readdirSync(out)) {
  if (/^LICENSE/i.test(e) && !fs.existsSync(path.join(dest, e))) fs.cpSync(path.join(out, e), path.join(dest, e));
}
if (!win) fs.chmodSync(path.join(dest, exe), 0o755);
fs.writeFileSync(marker, tag);
fs.rmSync(tmp, { recursive: true, force: true });
console.log(`llama.cpp ${tag} (${key}) ready in ${dest}`);
