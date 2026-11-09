import Foundation
import Observation
import UIKit
import UserNotifications

/// Instant notifications through Ferrowl's push server (push.ferrowl.com),
/// as on Android:
///
/// 1. the phone registers with Apple and gets a device token, which it
///    sends to the server, getting back a secret device token of its own;
/// 2. it creates a random 256-bit key, kept only in the Keychain, and
///    sends every account the server can watch (IMAP with a password;
///    after the first one only with Ferrowl Pro) sealed with that key;
/// 3. at every start it sends the key again (`unlock`): the server keeps it
///    in memory only.
///
/// The server then sends "New mail" through Apple when an inbox changes;
/// the message holds only an account id, never sender or contents.
///
/// Needs the Push Notifications capability (a paid Apple Developer team):
/// see `ios/README.md`.
@MainActor
@Observable
final class PushRegistrar {
    static let base = URL(string: "https://push.ferrowl.com")!
    private static let secrets = "com.ferrowl.app.push"
    /// Set once this installation has used the server (settings, unlike the
    /// Keychain, are removed with the app).
    private static let installedKey = "push.installed"

    private(set) var enabled: Bool {
        didSet { SharedStorage.defaults.set(enabled, forKey: "push.enabled") }
    }

    /// The user was told what instant notifications send to the server and
    /// answered (first launch, or the switch in Settings). Nothing goes to
    /// the server before.
    private(set) var asked: Bool {
        didSet { SharedStorage.defaults.set(asked, forKey: "push.asked") }
    }

    /// Server account ids by address (lowercase).
    private var accounts: [String: String] {
        didSet {
            if let data = try? JSONEncoder().encode(accounts) {
                SharedStorage.defaults.set(data, forKey: "push.accounts")
            }
        }
    }

    private var apnsToken: String?
    private var waiting: CheckedContinuation<String, Error>?
    private var running = false

    init() {
        // On by default: the phone registers with the first account; turning
        // it off in Settings is remembered.
        SharedStorage.defaults.register(defaults: ["push.enabled": true])
        enabled = SharedStorage.defaults.bool(forKey: "push.enabled")
        asked = SharedStorage.defaults.bool(forKey: "push.asked")
        accounts = (SharedStorage.defaults.data(forKey: "push.accounts"))
            .flatMap { try? JSONDecoder().decode([String: String].self, from: $0) } ?? [:]
    }

    // MARK: Apple

    /// The app delegate got the phone's token from Apple.
    func received(token: Data) {
        let hex = token.map { String(format: "%02x", $0) }.joined()
        apnsToken = hex
        waiting?.resume(returning: hex)
        waiting = nil
    }

    func failed(_ error: Error) {
        waiting?.resume(throwing: error)
        waiting = nil
    }

    private func appleToken() async throws -> String {
        if let apnsToken { return apnsToken }
        return try await withCheckedThrowingContinuation { continuation in
            waiting = continuation
            UIApplication.shared.registerForRemoteNotifications()
        }
    }

    // MARK: switch

    /// Turns instant notifications on. Throws with a message to show.
    func enable(_ app: AppModel) async throws {
        let watched = Self.watched(app)
        if watched.isEmpty {
            throw FerrowlError.Failed(message: t(app.mail.accounts.first?.protocol == .pop3 ? "push_pop3" : "push_oauth"))
        }
        let granted = try await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge])
        guard granted else { throw FerrowlError.Failed(message: t("push_error", ["err": t("notifications")])) }
        asked = true
        enabled = true
        do {
            try await sync(app)
        } catch {
            enabled = false
            throw error
        }
    }

    /// Turns them off and removes everything from the server.
    func disable() async {
        asked = true
        enabled = false
        if let token = Keychain.get(Self.secrets, "device") {
            _ = try? await call("DELETE", "/v1/device", token: token, body: nil)
        }
        Keychain.delete(Self.secrets, "device")
        Keychain.delete(Self.secrets, "key")
        accounts = [:]
    }

    // MARK: registration

    /// The accounts the server can watch.
    static func watched(_ app: AppModel) -> [Account] {
        app.mail.accounts.filter { app.mayUse($0.email) && $0.protocol == .imap && $0.oauth == nil }
    }

    /// Brings the registration up to date (start, accounts or Pro changed).
    func sync(_ app: AppModel) async throws {
        guard enabled, asked, !running else { return }
        running = true
        defer { running = false }
        let apple = try await appleToken()
        let body: [String: Any] = ["apns_token": apple, "apns_sandbox": Self.sandbox]
        var device = Keychain.get(Self.secrets, "device")
        // The Keychain outlives the app, its settings do not: a device left by
        // an earlier installation would keep the server watching accounts
        // this one knows nothing about. It is removed and the phone starts over.
        if let token = device, accounts.isEmpty, !SharedStorage.defaults.bool(forKey: Self.installedKey) {
            _ = try? await call("DELETE", "/v1/device", token: token, body: nil)
            Keychain.delete(Self.secrets, "device")
            Keychain.delete(Self.secrets, "key")
            device = nil
        }
        SharedStorage.defaults.set(true, forKey: Self.installedKey)
        if let token = device {
            do {
                _ = try await call("PUT", "/v1/device", token: token, body: body)
            } catch PushError.status(401, _) {
                // The server no longer knows this phone: register again.
                device = nil
                accounts = [:]
            }
        }
        if device == nil {
            let reply = try await call("POST", "/v1/devices", token: nil, body: body)
            guard let token = reply["device_token"] as? String else { throw PushError.status(0, "") }
            Keychain.set(Self.secrets, "device", token)
            Keychain.set(Self.secrets, "key", Self.newKey())
            device = token
        }
        guard let token = device, let key = Keychain.get(Self.secrets, "key") else { return }
        // Registrations kept by versions before the shared settings, which the
        // server may still watch next to newer ones: removed once.
        if let data = UserDefaults.standard.data(forKey: "push.accounts"),
           let legacy = try? JSONDecoder().decode([String: String].self, from: data) {
            for (email, id) in legacy where accounts[email] != id {
                _ = try? await call("DELETE", "/v1/accounts/\(id)", token: token, body: nil)
            }
            UserDefaults.standard.removeObject(forKey: "push.accounts")
        }
        if !accounts.isEmpty {
            _ = try await call("POST", "/v1/unlock", token: token, body: ["key": key])
        }
        let wanted = Self.watched(app)
        for (email, id) in accounts where !wanted.contains(where: { $0.email.lowercased() == email }) {
            _ = try? await call("DELETE", "/v1/accounts/\(id)", token: token, body: nil)
            accounts[email] = nil
        }
        for account in wanted where accounts[account.email.lowercased()] == nil {
            guard let json = try? JSONSerialization.jsonObject(with: Data(pushAccountJson(account: account).utf8)) else { continue }
            let reply = try await call("POST", "/v1/accounts", token: token, body: ["key": key, "account": json])
            if let id = reply["account_id"] as? String { accounts[account.email.lowercased()] = id }
        }
    }

    /// An account was set up again (maybe a new password): the server gets
    /// its new settings at the next sync.
    func forget(_ email: String) async {
        guard let id = accounts[email.lowercased()], let token = Keychain.get(Self.secrets, "device") else { return }
        _ = try? await call("DELETE", "/v1/accounts/\(id)", token: token, body: nil)
        accounts[email.lowercased()] = nil
    }

    /// The address of an account id the server sent.
    func email(of accountId: String) -> String? {
        accounts.first { $0.value == accountId }?.key
    }

    #if DEBUG
    /// Development builds get tokens from Apple's sandbox.
    private static let sandbox = true
    #else
    private static let sandbox = false
    #endif

    private static func newKey() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        return Data(bytes).base64EncodedString()
    }

    // MARK: HTTP

    enum PushError: Error {
        case status(Int, String)
    }

    private func call(_ method: String, _ path: String, token: String?, body: [String: Any]?) async throws -> [String: Any] {
        var request = URLRequest(url: Self.base.appendingPathComponent(path))
        request.httpMethod = method
        request.timeoutInterval = 30
        if let token { request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization") }
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }
        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        guard (200..<300).contains(status) else {
            let message = json["error"] as? String ?? "HTTP \(status)"
            throw PushError.status(status, message)
        }
        return json
    }
}

/// Text of a push registration failure, for the notice at the top.
func describePush(_ error: Error) -> String {
    if case PushRegistrar.PushError.status(_, let message) = error {
        return t("push_error", ["err": message])
    }
    if error is FerrowlError { return describe(error) }
    return t("push_error", ["err": error.localizedDescription])
}

/// Receives the phone's push token from Apple.
final class AppDelegate: NSObject, UIApplicationDelegate {
    var push: PushRegistrar?

    func application(_ application: UIApplication, didRegisterForRemoteNotificationsWithDeviceToken token: Data) {
        MainActor.assumeIsolated { push?.received(token: token) }
    }

    func application(_ application: UIApplication, didFailToRegisterForRemoteNotificationsWithError error: Error) {
        MainActor.assumeIsolated { push?.failed(error) }
    }

    /// A silent push from Ferrowl's server: mail of an account was read (or
    /// removed) elsewhere. Takes back the notifications of messages no
    /// longer unread and updates the number on the icon. iOS delivers these
    /// as it sees fit, and not after the app was swiped away; the app's
    /// own checks catch up then.
    func application(_ application: UIApplication, didReceiveRemoteNotification info: [AnyHashable: Any]) async
        -> UIBackgroundFetchResult {
        guard info["type"] as? String == "read",
              let account = MailNotifications.account(pushId: info["account"] as? String) else { return .noData }
        return await Self.clearRead(account) ? .newData : .failed
    }

    /// Reads which of `account`'s notified messages are still unread, on a
    /// connection of its own, and takes back the others.
    static func clearRead(_ account: Account) async -> Bool {
        let shown = await MailNotifications.delivered(for: account.email)
        let lastKey = "lastUid:\(account.email.lowercased())"
        let after = shown.uids.min().map { $0 - 1 } ?? UInt32(SharedStorage.defaults.integer(forKey: lastKey))
        let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
        guard let peek = try? await engine.peekInbox(account: account, after: after, limit: 0) else { return false }
        await MailNotifications.clearRead(account: account.email, unread: Set(peek.unreadUids),
                                          from: after + 1, noneUnread: peek.unread == 0)
        Badge.set(Int(peek.unread), for: account.email)
        Badge.show()
        return true
    }
}
