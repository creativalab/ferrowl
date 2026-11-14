# Ferrowl

A fast desktop email client written in Rust, with a native UI built on [iced](https://iced.rs), and native apps for iPhone and Android.

<img src="assets/icons/png/ferrowl-128.png" width="64" alt="Ferrowl icon">

## Features

- **Multiple IMAP + SMTP accounts**: switch from the sidebar, add/edit/remove in Settings (IMAP over TLS on 993; SMTP on 465 TLS or 587 STARTTLS), pure-Rust TLS (rustls)
- Three-pane window: folders · message list · reader
- Special folders detected automatically (Inbox, Sent, Drafts, Archive, Spam, Trash), unread counters, collapsible tree for all other folders
- List filters (All / Unread / Attachments) and search with `from:`, `has:attachment`, `is:unread`
- **Local cache + full-text search** (SQLite FTS5): folders open instantly from the cache, older messages are indexed in the background, and search covers sender, subject and text of every cached message (prefix matching, accents ignored)
- Reply, reply all, forward (with original attachments), archive, mark as unread — also from sticky buttons at the bottom of the message and from the **right-click menu** on any message (open, reply, reply all, forward, read/unread, archive, move to…, delete)
- **Drag and drop** messages onto a folder in the sidebar to move them
- **Delete moves to Trash**; deleting inside Trash asks for confirmation and removes the message for good
- Sent messages are saved to the Sent folder
- Compose with Cc/Bcc and file attachments
- Formatted messages: toolbar and shortcuts (`Ctrl B`/`I`/`K`) write Markdown, with a live preview; sent as `multipart/alternative` (HTML + plain text). Raw HTML is escaped and only `http`/`https`/`mailto`/`tel` links are kept. Can be switched to plain text only.
- **Attachment previews**: thumbnails for images and PDFs (first page), full preview of images, PDFs (rendered in pure Rust with [hayro](https://github.com/LaurenzV/hayro), up to 50 pages) and text files, with a *Download* button (saves to the Downloads folder)
- **OpenPGP** (pure Rust, [rPGP](https://github.com/rpgp/rpgp)): create or import a key in Settings › Security, then *Sign* / *Encrypt* in the composer. PGP/MIME (RFC 3156) out, PGP/MIME and inline PGP in, with badges for encrypted / signed / invalid signature / unknown key. Public keys travel automatically in the `Autocrypt` header; others can be imported from `.asc` files. Replies to encrypted mail are encrypted by default. Interoperability checked against GnuPG 2.4 (both directions, signatures and encryption)
- **HTML mail rendered like a browser** with [Blitz](https://github.com/DioxusLabs/blitz) (Firefox's Stylo CSS engine, no JavaScript): layout, colors, tables and images. Remote images can be turned off in Settings › Security (they let senders know when you open a message); `cid:` inline images always work. Links show their destination on hover and open in the browser; `mailto:` opens a new message. One click switches to plain text
- **6 color themes** (Ruggine, Ardesia, Bosco, Lavanda, Sabbia, Notte) defined in [`design/themes.json`](design/themes.json)
- **10 languages**: English, Italiano, Français, Deutsch, Español, Português, 中文, 日本語, 한국어, العربية (right-to-left layout). The system language is used by default, falling back to English.
- Keyboard shortcuts: `C` compose · `R` reply · `A` reply all · `F` forward · `E` archive · `U` mark unread · `J`/`K` or ↓/↑ next/previous · `Del` delete (to Trash) · `Shift Del` delete forever, after confirmation · `Ctrl K` or `/` search · `Ctrl Enter` send · `Esc` close
- **Push**: a second IMAP connection waits in IDLE on the inbox, so new mail shows up as soon as the server reports it, with a desktop notification (can be turned off in Settings › General). Servers without IDLE fall back to the refresh every 2 minutes

## Download

On iPhone (iOS 18 or later) and Android (8.0 or later), get Ferrowl from the stores:

<a href="https://apps.apple.com/app/ferrowl/id6831579655"><img src="assets/badges/app-store.svg" height="40" alt="Download on the App Store"></a>
<a href="https://play.google.com/store/apps/details?id=com.ferrowl.app"><img src="assets/badges/google-play.png" height="40" alt="Get it on Google Play"></a>

For computers, prebuilt packages are attached to each [release](https://github.com/creativalab/ferrowl/releases):

| System | Package |
|---|---|
| Linux x86_64 | `Ferrowl-x86_64.AppImage` (run directly: `chmod +x` and double-click), `ferrowl_<version>_amd64.deb` (Debian/Ubuntu: adds Ferrowl to the applications menu), `ferrowl-linux-x86_64.tar.gz` |
| Windows x86_64 | `ferrowl-windows-x86_64.zip` |
| macOS (Apple Silicon) | `Ferrowl-macos-arm64.dmg` (drag Ferrowl to Applications) |

The packages are not signed yet:
- **macOS**: right-click Ferrowl → Open the first time, or `xattr -dr com.apple.quarantine /Applications/Ferrowl.app`
- **Windows**: SmartScreen → "More info" → "Run anyway"

## Build from source

```bash
cargo run --release
```

Requires Rust 1.91+ (edition 2024). On Linux the build needs the fontconfig headers (`sudo apt install libfontconfig1-dev` on Debian/Ubuntu); at runtime only a graphics driver and fontconfig (present on every desktop) are needed. The app runs on Wayland and X11.

## Configuration

On first launch Ferrowl opens the account setup. Settings are stored in:

| OS | Path |
|---|---|
| Linux | `~/.config/ferrowl/config.toml` |
| macOS | `~/Library/Application Support/ferrowl/config.toml` |
| Windows | `%APPDATA%\ferrowl\config.toml` |

The local cache (message list, previews, text of opened messages, search index) is `cache.sqlite` in the data directory (`~/.local/share/ferrowl/` on Linux, `~/Library/Application Support/ferrowl/` on macOS, `%LOCALAPPDATA%\ferrowl\` on Windows), readable by the owner only. Deleting it is safe: it is rebuilt from the server.

OpenPGP keys live in `keys/` in the same data directory (owner-only): `own/` for your secret keys, `contacts/` for correspondents' public keys.

Passwords and OpenPGP key passphrases are kept in the system keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux: GNOME Keyring or KWallet). Where no keychain is available they stay in this file (permissions `600` on Linux/macOS).

For Gmail, use an [app password](https://support.google.com/accounts/answer/185833).

## Translations

UI strings live in [`core/locales/`](core/locales/), one TOML file per language. `core/locales/en.toml` is the reference: every other file must contain the same keys with the same `{placeholders}` — `cargo test` checks this.

To add a language: copy `en.toml`, translate it, and register it in `LANGS` in [`core/src/i18n.rs`](core/src/i18n.rs).

## Development

### Layout

The repository is a Cargo workspace:

- **`core/`** — `ferrowl-core`, the engine with no UI toolkit: IMAP, POP3 and SMTP, MIME, OpenPGP, the SQLite cache and search index, the POP3 mailbox, settings, secrets, translations. The mobile apps will use it too (see `docs/piano-mobile.md`). File locations come from `paths`; mobile apps set them with `ferrowl_core::paths::set`. Build it without the system keychain with `--no-default-features`.
- **root package** — `ferrowl`, the desktop app (iced): windows, views, HTML rendering (Blitz), attachment previews, push (IMAP IDLE), single instance, AppImage integration.

### Code quality

| Tool | Role (PHP / Node equivalent) | Command |
|---|---|---|
| rustfmt | formatter (PHP-CS-Fixer / Prettier) — config in `rustfmt.toml` | `cargo fmt` |
| clippy | linter (PHPStan / ESLint) | `cargo clippy --workspace --all-targets -- -D warnings` |
| cargo-deny | dependency advisories (composer audit / npm audit) — config in `deny.toml` | `cargo deny check advisories` |

- **Pull requests**: `checks.yml` runs formatting and cargo-deny; `build.yml` runs clippy and the tests on Linux (skipped when a PR only touches docs/Markdown). Optimized builds for Linux, Windows and macOS run on `master` and on `v*` tags (tags also publish the Release). Required checks: *Formatting (rustfmt)*, *Dependency advisories (cargo-deny)*, *Lints (clippy)*, *Test (Linux)*.
- **Pre-commit hook**: `.githooks/pre-commit` runs `cargo fmt --check` and clippy before each commit. Enable it once per clone with `git config core.hooksPath .githooks` (skip with `git commit --no-verify`).

### Tests

```bash
cargo test --workspace                       # unit tests (themes, locales, IMAP/POP3 helpers)
cargo test -- --ignored                      # network tests (TLS handshake with imap.gmail.com)
cargo test live_account -- --ignored --nocapture   # uses the configured account
```

Environment variables useful during development:

| Variable | Effect |
|---|---|
| `FERROWL_LANG=ar` | Force a UI language for this run |
| `FERROWL_THEME=notte` | Force a theme for this run |
| `FERROWL_DEV_VIEW=settings\|appearance\|general\|security\|compose\|popover` | Open a view directly |
| `FERROWL_SCREENSHOT=out.png` | Capture the window after `FERROWL_SCREENSHOT_DELAY` seconds (default 8) and quit |
| `FERROWL_SCREENSHOT_OPEN=1` | Open the first message before capturing (`FERROWL_DEV_OPEN_NTH=n` for another row, `FERROWL_DEV_PREVIEW_ATT=1` to preview its first attachment) |
| `FERROWL_SCREENSHOT_COUNT=n` | Take n captures (`out.png`, `out.2.png`, …) before quitting |
| `FERROWL_DEV_MENU=1` | Open the context menu on the second message |
| `FERROWL_DEV_CONFIRM=1` | Show the "delete forever?" dialog for the open message |
| `FERROWL_DEV_PREVIEW=1` | With `FERROWL_DEV_VIEW=compose`, show the formatted preview |

The design handoff (mockups, tokens, icon) is in [`design/`](design/).

## Roadmap

- Password in the system keychain
- Search across all folders
- Optional image loading per sender
- Tray icon, notification actions (reply/archive from the notification)
- OpenPGP: protected subject (memory hole), key passphrase in the system keychain
- Unified inbox across accounts, JMAP

## Support

Problems or questions: **support@ferrowl.com**

## License

[MIT](LICENSE). Bundled fonts: Inter, JetBrains Mono and Noto Sans Arabic, all under the SIL Open Font License (see `assets/fonts/`).
