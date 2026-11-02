# Contributing

Thanks for helping. Bug reports, fixes and new ideas are all welcome.

## Before you start

- Open an issue first for anything bigger than a small fix, so we can agree on the approach.
- The app is Windows only for now. Changes should keep the Windows build working.

## Set up

```powershell
npm install
npm run fetch:llama
npm run dev
```

## Before you open a pull request

```powershell
cd src-tauri
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same two commands. If you change behaviour, setup or settings, update the README, the matching page in
`docs/wiki`, and `CHANGELOG.md` in the same pull request.

## Licensing of contributions

Companion is licensed under Apache-2.0. Unless you say otherwise, any contribution you submit is licensed under the
same terms (section 5 of the license). Don't submit code you don't have the right to share, and don't paste in code
from projects with incompatible licenses.

## Security problems

Don't open a public issue for a vulnerability. Use GitHub's private "Report a vulnerability" option on the Security tab.
