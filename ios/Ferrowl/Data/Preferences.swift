import Foundation
import Observation

/// App preferences that are not about an account (UserDefaults).
@MainActor
@Observable
final class Preferences {
    private let defaults = UserDefaults.standard

    /// Lists grouped into conversations (the default) or one row per message.
    var threaded: Bool {
        didSet { defaults.set(threaded, forKey: "threaded") }
    }

    /// Order of message lists (newest first by default).
    var sort: SortOrder {
        didSet { defaults.set(Self.name(of: sort), forKey: "sort") }
    }

    /// Color theme id from `design/themes.json`, or "auto" (Ruggine, Notte
    /// when the phone is in dark mode).
    var theme: String {
        didSet { defaults.set(theme, forKey: "theme") }
    }

    /// Remote images in HTML mail (they tell senders the message was opened).
    var remoteImages: Bool {
        didSet { defaults.set(remoteImages, forKey: "remoteImages") }
    }

    /// New-mail notifications from the background refresh.
    var notifications: Bool {
        didSet { defaults.set(notifications, forKey: "notifications") }
    }

    /// The inbox's messages in the iPhone's search (Spotlight).
    var spotlight: Bool {
        didSet {
            defaults.set(spotlight, forKey: "spotlight")
            if !spotlight { SpotlightIndex.removeAll() }
        }
    }

    init() {
        defaults.register(defaults: ["threaded": true, "remoteImages": true, "notifications": true, "spotlight": true])
        threaded = defaults.bool(forKey: "threaded")
        sort = Self.order(named: defaults.string(forKey: "sort")) ?? .newest
        theme = defaults.string(forKey: "theme") ?? "auto"
        remoteImages = defaults.bool(forKey: "remoteImages")
        notifications = defaults.bool(forKey: "notifications")
        spotlight = defaults.bool(forKey: "spotlight")
    }

    static let orders: [SortOrder] = [.newest, .oldest, .largest, .smallest, .senderAz, .senderZa, .subjectAz]

    private static func name(of order: SortOrder) -> String {
        switch order {
        case .newest: "newest"
        case .oldest: "oldest"
        case .largest: "largest"
        case .smallest: "smallest"
        case .senderAz: "sender_az"
        case .senderZa: "sender_za"
        case .subjectAz: "subject_az"
        }
    }

    private static func order(named name: String?) -> SortOrder? {
        orders.first { self.name(of: $0) == name }
    }

    /// The translation key of a sort order's name.
    static func label(of order: SortOrder) -> String { "sort_" + name(of: order) }
}
