import Foundation
import UserNotifications

/// Runs when a push from Ferrowl's server arrives ("New mail" for an
/// account id): reads the account's inbox here on the phone and shows the
/// sender and subject of the new message (several: how many, and the
/// newest; none new: the newest message), with the unread count on the
/// app icon. The server never sees any of this. If reading takes too long
/// or fails, the plain "New mail" is shown.
final class NotificationService: UNNotificationServiceExtension {
    /// Delivered once, by whichever ends first: the work, the extension's
    /// own timeout or the system's.
    private let delivery = Delivery()
    private var work: Task<Void, Never>?

    /// Well inside the system's 30 seconds (Android allows 15): a server
    /// that does not answer gets the plain "New mail" in time.
    private static let timeout: Duration = .seconds(20)

    override func didReceive(_ request: UNNotificationRequest,
                             withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        let delivery = delivery
        delivery.start(content, handler)
        let accountId = request.content.userInfo["account"] as? String
        work = Task {
            await Self.fill(content, accountId: accountId, delivery: delivery)
            delivery.finish()
        }
        Task {
            try? await Task.sleep(for: Self.timeout)
            delivery.finish()
        }
    }

    override func serviceExtensionTimeWillExpire() {
        work?.cancel()
        delivery.finish()
    }

    /// The content handler, called once. Once it was, nothing is written
    /// any more (the notification, the shared counts, the memory of what
    /// was announced): what is shown would no longer match.
    final class Delivery: @unchecked Sendable {
        private let lock = NSLock()
        private var content: UNMutableNotificationContent?
        private var handler: ((UNNotificationContent) -> Void)?

        func start(_ content: UNMutableNotificationContent, _ handler: @escaping (UNNotificationContent) -> Void) {
            lock.lock()
            self.content = content
            self.handler = handler
            lock.unlock()
        }

        /// Runs `write` only while not delivered yet (delivery waits for it).
        func whilePending(_ write: () -> Void) {
            lock.lock()
            defer { lock.unlock() }
            if handler != nil { write() }
        }

        func finish() {
            lock.lock()
            let handler = handler
            let content = content
            self.handler = nil
            lock.unlock()
            if let handler, let content { handler(content) }
        }
    }

    private static func fill(_ content: UNMutableNotificationContent, accountId: String?, delivery: Delivery) async {
        guard let account = MailNotifications.account(pushId: accountId) else { return }
        delivery.whilePending {
            content.subtitle = account.email
            content.threadIdentifier = account.email
            // Focus filters keep the notifications of the accounts they show.
            content.filterCriteria = account.email.lowercased()
        }

        setTokenVault(vault: KeychainTokenVault())
        let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
        do {
            // The same memory as the app's background check: each message once.
            let key = "lastUid:\(account.email.lowercased())"
            let last = UInt32(SharedStorage.defaults.integer(forKey: key))
            // Only what the notification shows: an extension has about 30
            // seconds and little memory, too little for loading the inbox.
            let peek = try await engine.peekInbox(account: account, after: last, limit: 1)
            guard !Task.isCancelled else { return }
            // The user's rules may keep messages quiet ("silent").
            let silenced = Set(engine.silenced(account: account, headers: peek.fresh))
            delivery.whilePending {
                content.badge = NSNumber(value: Badge.set(Int(peek.unread), for: account.email, fresh: peek.fresh))
                let newest = peek.newest?.uid ?? last
                SharedStorage.defaults.set(Int(max(newest, last)), forKey: key)
                show(peek, last: last, silenced: silenced, account: account, in: content)
            }
        } catch {
            // The plain "New mail" stays.
        }
    }

    private static func show(_ peek: InboxPeek, last: UInt32, silenced: Set<UInt32>, account: Account,
                             in content: UNMutableNotificationContent) {
        // First time for this account: no memory of what was already
        // announced, so only the newest unread message is shown.
        let count = last == 0 ? min(peek.freshCount, 1) : peek.freshCount
        // Nothing unread is new (the open app already saw it, or it was
        // read elsewhere): the newest message rather than a bare "New mail".
        guard var first = peek.fresh.first ?? peek.newest else { return }
        // Shown without sound or banner when every new one is kept quiet.
        let loud = peek.fresh.filter { !silenced.contains($0.uid) }
        if !peek.fresh.isEmpty, loud.isEmpty {
            content.sound = nil
            content.interruptionLevel = .passive
        } else if let shown = loud.first {
            first = shown
        }
        // Tapping it opens this message.
        content.userInfo["email"] = account.email
        content.userInfo["uid"] = NSNumber(value: first.uid)
        let sender = first.from.isEmpty ? first.fromAddress : first.from
        let subject = first.subject.isEmpty ? t("no_subject") : first.subject
        if count <= 1 {
            content.title = sender
            content.body = subject
        } else {
            content.title = t("notif_many", ["n": "\(count)"])
            content.body = "\(sender): \(subject)"
        }
    }
}
