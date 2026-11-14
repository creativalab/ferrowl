import BackgroundTasks
import Foundation
import UserNotifications

/// New-mail notifications without a server: iOS wakes the app now and then
/// (`BGAppRefreshTask`; how often is up to iOS, from minutes to hours,
/// depending on how much the app is used), and while it is open it checks
/// every 3 minutes. The inbox of every account is checked and new messages
/// are announced with a local notification (one thread per account).
///
/// The newest uid seen is kept per account, so a message is announced once.
@MainActor
enum BackgroundRefresh {
    nonisolated static let task = "com.ferrowl.app.refresh"

    /// Registers the task; must run before the app finishes launching.
    static func register(_ app: AppModel) {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: task, using: nil) { task in
            guard let task = task as? BGAppRefreshTask else { return }
            // Told once: when the work ends, or at once when iOS says time
            // is up (engine calls do not stop on cancel, and answering late
            // gets the app killed and woken less often).
            let finish = Once { ok in task.setTaskCompleted(success: ok) }
            let work = Task { @MainActor in
                let ok = await check(app)
                schedule()
                finish.call(ok)
            }
            task.expirationHandler = {
                work.cancel()
                schedule()
                finish.call(false)
            }
        }
    }

    /// Asks iOS to wake the app again in about 15 minutes (at the earliest).
    nonisolated static func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: task)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(request)
    }

    static func askPermission() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
    }

    /// Checks the inbox of every account that may be used (the first one,
    /// all with Ferrowl Pro); returns false when a server could not be
    /// reached. With the app open (`foreground`) the active account is not
    /// announced: its list is on screen.
    static func check(_ app: AppModel, foreground: Bool = false) async -> Bool {
        let mail = app.mail
        guard app.prefs.notifications, !checking else { return true }
        checking = true
        defer { checking = false }
        // Woken in the background, the screen's App Store check may not
        // have run: Ferrowl Pro decides which accounts are checked.
        await app.pro.ready()
        var ok = true
        for account in mail.accounts where app.mayUse(account.email) {
            guard !Task.isCancelled else { break }
            let active = account.email == mail.activeEmail
            do {
                let headers: [Header]
                let unread: UInt32?
                if active {
                    try await mail.ensureConnected()
                    var inbox = try await mail.headers(mail.inboxId, on: account.email)
                    // The user's rules first: what they move or mark is not announced.
                    if let done = await mail.applyRules(mail.inboxId, inbox, on: account.email) {
                        inbox.removeAll { done.removed.contains($0.uid) || done.read.contains($0.uid) }
                    }
                    headers = inbox
                    unread = try? await mail.engine.unread(folderIds: [mail.inboxId])[mail.inboxId]
                } else {
                    // A second engine: the account on screen stays connected.
                    let folders = try await app.side.connect(account: account)
                    let inbox = folders.first { $0.kind == .inbox }?.id ?? "INBOX"
                    headers = try await app.side.headers(folderId: inbox).headers
                    unread = try? await app.side.unread(folderIds: [inbox])[inbox]
                }
                // Messages a rule keeps quiet are not announced.
                let quiet = Set(mail.engine.silenced(account: account, headers: headers))
                notifyNew(headers, account: account.email, announce: !(foreground && active), quiet: quiet)
                // Notifications of messages read since (here or elsewhere) go.
                if let oldest = headers.map(\.uid).min() {
                    let unread = Set(headers.filter { !$0.seen }.map(\.uid))
                    await MailNotifications.clearRead(account: account.email, unread: unread,
                                                      from: oldest, noneUnread: unread.isEmpty)
                }
                // The number on the app icon: the inbox's unread messages.
                if let n = Badge.unread(server: unread, headers: headers, gmail: Badge.isGmail(account)) {
                    let inbox = Badge.isGmail(account) ? headers.filter { $0.category == 0 } : headers
                    Badge.set(n, for: account.email, recent: inbox)
                }
                Badge.show()
                if active { mail.touch() }
            } catch {
                ok = false
            }
        }
        return ok
    }

    /// A check is running (background task and the open app's timer).
    private static var checking = false

    /// While the app is open: checks every few minutes.
    static func watch(_ app: AppModel) async {
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(180))
            guard !Task.isCancelled else { return }
            _ = await check(app, foreground: true)
        }
    }

    /// Remembers the newest message of the inbox (and announces the unread
    /// ones newer than the last time when `announce`).
    static func notifyNew(_ headers: [Header], account: String, announce: Bool, quiet: Set<UInt32> = []) {
        let key = "lastUid:\(account.lowercased())"
        let defaults = SharedStorage.defaults
        let last = UInt32(defaults.integer(forKey: key))
        let newest = headers.map(\.uid).max() ?? last
        defaults.set(Int(max(newest, last)), forKey: key)
        // First check for this account: only remember where we are.
        guard announce, last > 0 else { return }
        let fresh = headers.filter { $0.uid > last && !$0.seen && !quiet.contains($0.uid) }.sorted { $0.uid > $1.uid }
        guard let first = fresh.first else { return }
        let content = UNMutableNotificationContent()
        if fresh.count == 1 {
            content.title = first.from
            content.body = first.subject.isEmpty ? t("no_subject") : first.subject
        } else {
            content.title = t("notif_new_mail")
            content.body = t("notif_many", ["n": "\(fresh.count)"])
        }
        content.subtitle = account
        content.sound = .default
        content.badge = NSNumber(value: Badge.total())
        content.threadIdentifier = account
        content.filterCriteria = account.lowercased()
        // Tapping it opens the newest message.
        content.userInfo = ["email": account, "uid": NSNumber(value: first.uid)]
        let request = UNNotificationRequest(identifier: "mail:\(account):\(first.uid)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}

/// Calls its function the first time only (from any thread).
final class Once: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false
    private let action: (Bool) -> Void

    init(_ action: @escaping (Bool) -> Void) { self.action = action }

    func call(_ value: Bool) {
        lock.lock()
        let first = !done
        done = true
        lock.unlock()
        if first { action(value) }
    }
}
