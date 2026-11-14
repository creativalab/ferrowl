# Ferrowl for iOS

SwiftUI app (iPhone and iPad, iOS 18+) on top of the Rust engine (`../core`,
bound with UniFFI in `../ffi`), the same engine as the desktop and Android
apps.

## Layout

- `Ferrowl/` — the app (folder synchronized with the Xcode project: new
  files are picked up automatically)
  - `Data/` — `MailRepository` (engine calls, accounts), `AccountStore`
    (settings in a JSON file, passwords in the Keychain), `Keychain`
    (also the engine's token vault), `MicrosoftSignIn`
    (`ASWebAuthenticationSession`), `ProStore` (StoreKit 2, product
    `ferrowl_pro`), `BackgroundRefresh` (`BGAppRefreshTask` + local
    notifications), `L10n` (texts from `core/locales` through the engine)
  - `UI/` — setup, mailbox (`NavigationSplitView`: three columns on iPad,
    a stack on iPhone), message (`WKWebView` with JavaScript off and a
    strict CSP), conversation, compose, settings, contacts, calendar
- `Engine/` — **generated, not committed**, by `../ffi/build-ios.sh`:
  `Swift/ferrowl_ffi.swift`, and the Rust library the project links
  (`lib/<platform>/`, `include/`). The project's *Engine* target builds it
  before the app, so the library and the Swift bindings always match.
- `Config/Info.plist` — languages, background refresh

## Requirements

- Xcode 16 or newer
- Rust: `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`

## Build

Once, before opening the project (Xcode needs the generated Swift file
when it opens the project):

```bash
ffi/build-ios.sh debug
```

Then open `ios/Ferrowl.xcodeproj` and press Run. The *Engine* target
recompiles the Rust engine for the platform being built when its sources
change. (`ffi/build-ios.sh release` also makes `FerrowlCore.xcframework`,
for use outside this project.)

From the command line:

```bash
cd ios
xcodebuild -scheme Ferrowl -destination 'platform=iOS Simulator,name=iPhone 17 Pro' build
```

On a device, set your team in *Signing & Capabilities*. The app has the
Push Notifications capability (`Config/Ferrowl.entitlements`), which only a
paid team can sign. With a free (personal) team leave it out and use another
bundle id, so `com.ferrowl.app` stays free for the App Store account:

```bash
xcodebuild -scheme Ferrowl -destination 'id=<UDID>' -allowProvisioningUpdates \
  DEVELOPMENT_TEAM=<TEAM> PRODUCT_BUNDLE_IDENTIFIER=com.ferrowl.app.dev \
  CODE_SIGN_ENTITLEMENTS= build
```

## Still to set up

- **Sign in with Microsoft**: register `msauth.com.ferrowl.app://auth` as an
  iOS/macOS redirect on the Ferrowl app in Microsoft Entra.
- **Sign in with Google**: needs an iOS OAuth client; until then Gmail uses
  an app password.
- **Ferrowl Pro**: create the `ferrowl_pro` yearly subscription in App Store
  Connect (and the In-App Purchase capability, which needs a paid team).
- **Instant notifications** (Settings › Instant notifications) go through
  push.ferrowl.com and Apple (APNs). They need the paid team:
  1. the project already has the **Push Notifications** capability; set the
     paid team in *Signing & Capabilities*;
  2. on the server, the APNs `.p8` key with its Key ID and Team ID (see
     `server/README.md`).
  Until then the switch fails with a message and the app keeps using the
  background refresh. The push says only "New mail" (translated:
  `Localizable.strings`, generated from `core/locales`); the notification
  service extension (`FerrowlNotifications/`) replaces it with sender and
  subject, reading only what it shows (`Engine.peekInbox`) to fit in the
  extension's time and memory. When mail is read elsewhere the server sends a
  silent background push and the app takes back those notifications
  (`AppDelegate`, `Shared/MailNotifications`); iOS does not deliver these
  to an app swiped away, so the app's own checks also clean up.

## Security notes

- Passwords, OpenPGP passphrases and sign-in tokens are in the Keychain,
  this device only (`AfterFirstUnlockThisDeviceOnly`); settings and cache
  are left out of backups.
- HTML mail: JavaScript off, non-persistent web data, Content-Security-Policy
  allowing only inline styles and `data:` images (remote images when
  allowed in Settings); links open in Safari.
