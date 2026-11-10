import Foundation
import UserNotifications

/// The number on the app icon: unread messages in the inboxes of every
/// account (Gmail: of the Primary tab, as the Gmail app counts). Each
/// account's count is kept in the shared settings, written by the app when
/// it loads an inbox and by the notification extension when new mail
/// arrives; accounts no longer on the phone do not count.
enum Badge {
    /// An account's count: the server's unread count of the inbox, or for
    /// Gmail the unread messages of the Primary tab among the newest ones
    /// (the server's count includes Promotions, Social, Updates, Forums).
    static func unread(server: UInt32?, headers: [Header], gmail: Bool) -> Int? {
        if gmail { return headers.filter { !$0.seen && $0.category == 0 }.count }
        return server.map { Int($0) }
    }

    static func isGmail(_ account: Account) -> Bool {
        account.incomingHost.lowercased().hasSuffix("gmail.com")
    }

    private static func key(_ email: String) -> String { "unread:\(email.lowercased())" }

    /// The total, after saving one account's count. The widget gets it too,
    /// with the newest unread messages when `recent` (inbox headers) is
    /// given, or just the `fresh` ones a notification announced.
    @discardableResult
    static func set(_ unread: Int, for email: String, recent: [Header]? = nil, fresh: [Header]? = nil) -> Int {
        let d = SharedStorage.defaults
        d.set(unread, forKey: key(email))
        let mails = { (headers: [Header]) in
            headers.filter { !$0.seen }.map {
                WidgetMail(from: $0.from.isEmpty ? $0.fromAddress : $0.from, subject: $0.subject,
                           date: Date(timeIntervalSince1970: TimeInterval($0.timestamp)), uid: $0.uid,
                           link: messageLink(account: email, folder: "INBOX", uid: $0.uid))
            }
        }
        if let fresh {
            WidgetData.add(email: email, unread: unread, fresh: mails(fresh))
        } else {
            WidgetData.update(email: email, unread: unread, recent: recent.map(mails))
        }
        #if DEBUG
        print("badge: \(email) = \(unread)")
        #endif
        return total()
    }

    /// One account's last known count (0 when not known yet).
    static func count(for email: String) -> Int {
        SharedStorage.defaults.integer(forKey: key(email))
    }

    static func total() -> Int {
        let d = SharedStorage.defaults
        let accounts = Set(AccountStore(directory: SharedStorage.directory).load().accounts.map { $0.email.lowercased() })
        var sum = 0
        for (key, value) in d.dictionaryRepresentation() where key.hasPrefix("unread:") {
            // A removed account (also one removed before this was kept tidy).
            guard accounts.contains(String(key.dropFirst("unread:".count))) else {
                d.removeObject(forKey: key)
                continue
            }
            sum += value as? Int ?? 0
        }
        return sum
    }

    /// Forgets a removed account.
    static func forget(_ email: String) {
        SharedStorage.defaults.removeObject(forKey: key(email))
        WidgetData.keep(AccountStore(directory: SharedStorage.directory).load().accounts.map(\.email))
    }

    /// Shows the total on the icon (the app; the extension sets it in the
    /// notification it shows).
    static func show() {
        UNUserNotificationCenter.current().setBadgeCount(total())
    }
}
