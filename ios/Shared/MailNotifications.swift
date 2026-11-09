import Foundation
import UserNotifications

/// The new-mail notifications on screen: which account a push is about,
/// and taking back the notifications of messages read (or removed)
/// elsewhere, e.g. in Ferrowl on the computer.
enum MailNotifications {
    /// The account of a push: the server sends the id it gave the account.
    static func account(pushId: String?) -> Account? {
        let ids = SharedStorage.defaults.data(forKey: "push.accounts")
            .flatMap { try? JSONDecoder().decode([String: String].self, from: $0) } ?? [:]
        guard let pushId, let email = ids.first(where: { $0.value == pushId })?.key else { return nil }
        return AccountStore(directory: SharedStorage.directory).load().accounts
            .first { $0.email.lowercased() == email }
    }

    /// The UIDs of `account`'s notifications on screen (those that name
    /// their message), and whether there is one that does not.
    static func delivered(for account: String) async -> (uids: [UInt32], unnamed: Bool) {
        var uids: [UInt32] = []
        var unnamed = false
        for n in await UNUserNotificationCenter.current().deliveredNotifications() where isOf(n, account) {
            if let uid = uid(of: n) { uids.append(uid) } else { unnamed = true }
        }
        return (uids, unnamed)
    }

    /// Removes `account`'s notifications whose message has a UID of at
    /// least `from` and is not among `unread`. One that names no message
    /// (the inbox could not be read when it came) goes once nothing in the
    /// inbox is unread (`noneUnread`).
    static func clearRead(account: String, unread: Set<UInt32>, from: UInt32, noneUnread: Bool) async {
        let center = UNUserNotificationCenter.current()
        let gone = await center.deliveredNotifications()
            .filter { n in
                guard isOf(n, account) else { return false }
                guard let uid = uid(of: n) else { return noneUnread }
                return uid >= from && !unread.contains(uid)
            }
            .map(\.request.identifier)
        if !gone.isEmpty { center.removeDeliveredNotifications(withIdentifiers: gone) }
    }

    private static func isOf(_ n: UNNotification, _ account: String) -> Bool {
        let content = n.request.content
        let email = content.userInfo["email"] as? String ?? content.threadIdentifier
        return email.caseInsensitiveCompare(account) == .orderedSame
    }

    private static func uid(of n: UNNotification) -> UInt32? {
        let info = n.request.content.userInfo
        return (info["uid"] as? NSNumber)?.uint32Value ?? (info["uid"] as? Int).map { UInt32($0) }
    }
}
