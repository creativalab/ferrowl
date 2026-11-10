import Foundation
import UserNotifications

/// Runs when a push from Ferrowl's server arrives ("New mail" for an
/// account id): reads the account's inbox here on the phone and shows the
/// sender and subject of the new message (several: how many, and the
/// newest; none new: the newest message), with the unread count on the
/// app icon. The server never sees any of this. If reading takes too long
/// or fails, the plain "New mail" is shown.
final class NotificationService: UNNotificationServiceExtension {
    private var deliver: ((UNNotificationContent) -> Void)?
    private var content: UNMutableNotificationContent?
    private var work: Task<Void, Never>?

    override func didReceive(_ request: UNNotificationRequest,
                             withContentHandler handler: @escaping (UNNotificationContent) -> Void) {
        deliver = handler
        let content = (request.content.mutableCopy() as? UNMutableNotificationContent) ?? UNMutableNotificationContent()
        self.content = content
        let accountId = request.content.userInfo["account"] as? String
        work = Task {
            await Self.fill(content, accountId: accountId)
            self.finish()
        }
    }

    override func serviceExtensionTimeWillExpire() {
        work?.cancel()
        finish()
    }

    private func finish() {
        guard let deliver, let content else { return }
        self.deliver = nil
        deliver(content)
    }

    private static func fill(_ content: UNMutableNotificationContent, accountId: String?) async {
        guard let account = MailNotifications.account(pushId: accountId) else { return }
        content.subtitle = account.email
        content.threadIdentifier = account.email

        setTokenVault(vault: KeychainTokenVault())
        let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
        do {
            // The same memory as the app's background check: each message once.
            let key = "lastUid:\(account.email.lowercased())"
            let last = UInt32(SharedStorage.defaults.integer(forKey: key))
            // Only what the notification shows: an extension has about 30
            // seconds and little memory, too little for loading the inbox.
            let peek = try await engine.peekInbox(account: account, after: last, limit: 1)
            content.badge = NSNumber(value: Badge.set(Int(peek.unread), for: account.email, fresh: peek.fresh))
            let newest = peek.newest?.uid ?? last
            SharedStorage.defaults.set(Int(max(newest, last)), forKey: key)
            // First time for this account: no memory of what was already
            // announced, so only the newest unread message is shown.
            let count = last == 0 ? min(peek.freshCount, 1) : peek.freshCount
            // Nothing unread is new (the open app already saw it, or it was
            // read elsewhere): the newest message rather than a bare "New mail".
            guard let first = peek.fresh.first ?? peek.newest else { return }
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
        } catch {
            // The plain "New mail" stays.
        }
    }
}
