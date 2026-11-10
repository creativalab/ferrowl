import Foundation
import WidgetKit

/// A message the Home Screen widget shows.
struct WidgetMail: Codable, Hashable {
    let from: String
    let subject: String
    let date: Date
    let uid: UInt32
    /// Its `ferrowl://open` link: tapping it opens the message.
    let link: String
}

/// An account as the widget shows it: its inbox's unread count and the
/// newest unread messages.
struct WidgetAccount: Codable, Hashable {
    let email: String
    var unread: Int
    var recent: [WidgetMail]
}

/// What the app and its notification extension leave for the widget in the
/// App Group (no engine in the widget: it only reads this), and the
/// widget's reload when it changes.
enum WidgetData {
    static let kind = "FerrowlUnread"
    private static let key = "widget.accounts"
    private static let defaults = UserDefaults(suiteName: "group.com.ferrowl.app") ?? .standard
    /// Newest unread messages kept per account.
    static let recentLimit = 5

    static func load() -> [WidgetAccount] {
        guard let data = defaults.data(forKey: key),
              let accounts = try? JSONDecoder().decode([WidgetAccount].self, from: data) else { return [] }
        return accounts
    }

    private static func save(_ accounts: [WidgetAccount]) {
        guard let data = try? JSONEncoder().encode(accounts), data != defaults.data(forKey: key) else { return }
        defaults.set(data, forKey: key)
        WidgetCenter.shared.reloadTimelines(ofKind: kind)
    }

    /// An account's count, and its newest unread messages when known
    /// (`nil` keeps those the widget has, minus any once there is no unread).
    static func update(email: String, unread: Int, recent: [WidgetMail]?) {
        var accounts = load()
        let i = accounts.firstIndex { $0.email.caseInsensitiveCompare(email) == .orderedSame }
        var account = i.map { accounts[$0] } ?? WidgetAccount(email: email, unread: 0, recent: [])
        account.unread = unread
        if let recent {
            account.recent = Array(recent.sorted { $0.date > $1.date }.prefix(recentLimit))
        } else if unread == 0 {
            account.recent = []
        }
        if let i { accounts[i] = account } else { accounts.append(account) }
        save(accounts)
    }

    /// New messages just notified: put in front of the ones the widget has.
    static func add(email: String, unread: Int, fresh: [WidgetMail]) {
        let old = load().first { $0.email.caseInsensitiveCompare(email) == .orderedSame }?.recent ?? []
        let known = Set(fresh.map(\.uid))
        update(email: email, unread: unread, recent: fresh + old.filter { !known.contains($0.uid) })
    }

    /// Mail read elsewhere: of the messages from UID `from` on, only those
    /// still unread stay on the widget.
    static func dropRead(email: String, unread: Set<UInt32>, from: UInt32) {
        var accounts = load()
        guard let i = accounts.firstIndex(where: { $0.email.caseInsensitiveCompare(email) == .orderedSame }) else { return }
        accounts[i].recent.removeAll { $0.uid >= from && !unread.contains($0.uid) }
        save(accounts)
    }

    /// A message read or archived from the widget: it leaves at once.
    static func removeMail(link: String) {
        var accounts = load()
        for i in accounts.indices where accounts[i].recent.contains(where: { $0.link == link }) {
            accounts[i].recent.removeAll { $0.link == link }
            accounts[i].unread = max(0, accounts[i].unread - 1)
        }
        save(accounts)
    }

    /// The accounts on the phone, in their order: others are dropped.
    static func keep(_ emails: [String]) {
        let accounts = load()
        let ordered = emails.compactMap { e in accounts.first { $0.email.caseInsensitiveCompare(e) == .orderedSame } }
        if ordered != accounts { save(ordered) }
    }
}
