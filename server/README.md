# ferrowl-push

Push relay for the Ferrowl mobile apps. For each registered account it keeps
an IMAP IDLE connection on the inbox and, when new unread mail arrives, wakes the
phone with a message that contains only an account id: a **data-only**
Firebase Cloud Messaging message on Android, an Apple Push Notification
service alert ("New mail", translated by the phone) on iPhone. The app then
fetches the mail directly from the mail server.

## Privacy and security

- Message contents are never read or stored; only the inbox message count is
  watched, and which inbox messages are unread (their UIDs, in memory: an
  IMAP search on the `\Seen` flag). Mail already read when it arrives wakes
  nobody; when unread mail is read or removed elsewhere the phone gets a
  silent message (Android: data-only, normal priority; iPhone: background
  push) so it can take back its notifications.
- Account settings (including the password) are stored sealed with
  AES-256-GCM under a 256-bit key generated and kept by the phone. The server
  holds the key **in memory only**; after a restart it waits for each phone to
  send it again (`POST /v1/unlock`, done by the app at every sync).
- Device tokens are stored as SHA-256 hashes.
- `DELETE /v1/device` removes the device and all its accounts.
- POP3 accounts are refused (no IDLE).
- Logs carry account ids and the first line of connection errors; never
  passwords or message data.

## API

See the table at the top of [`src/api.rs`](src/api.rs). All calls except
registration need `Authorization: Bearer <device_token>`.

## Run locally

```bash
FERROWL_PUSH_DB=/tmp/push.sqlite cargo run -p ferrowl-push
```

Without credentials (Firebase, APNs) notifications are only logged.

## Deploy (VPS)

1. Build: `cargo build --release -p ferrowl-push` (or on the VPS).
2. Firebase console → Project settings → Service accounts → *Generate new
   private key*; save it as `/etc/ferrowl-push/firebase.json`, owner
   `ferrowl-push`, mode `600`.
3. Apple Developer site → Certificates, Identifiers & Profiles → Keys → *+*,
   enable *Apple Push Notifications service (APNs)*, download the `.p8` key
   (it can be downloaded only once) and note its Key ID and the Team ID;
   save it as `/etc/ferrowl-push/apns.p8`, owner `ferrowl-push`, mode `600`.
   Never commit it.
4. Install [`deploy/ferrowl-push.service`](deploy/ferrowl-push.service)
   (instructions in the file) and Caddy with
   [`deploy/Caddyfile`](deploy/Caddyfile) for HTTPS.
5. Point the DNS name (e.g. `push.ferrowl.com`) at the VPS.

| Variable | Default | Meaning |
|---|---|---|
| `FERROWL_PUSH_DB` | `ferrowl-push.sqlite` | SQLite database |
| `FERROWL_PUSH_BIND` | `127.0.0.1:8080` | listen address (behind Caddy) |
| `GOOGLE_APPLICATION_CREDENTIALS` | — | Firebase service account key (Android) |
| `FERROWL_APNS_KEY` | — | APNs `.p8` key (iPhone) |
| `FERROWL_APNS_KEY_ID` | — | its Key ID |
| `FERROWL_APNS_TEAM_ID` | — | Apple Developer Team ID |
| `FERROWL_APNS_TOPIC` | `com.ferrowl.app` | the iOS app's bundle id |
