import Foundation

/// The accounts the current Focus shows (Settings › Focus › a Focus ›
/// Filters › Ferrowl). Kept in the App Group: the notification extension
/// reads it too. Nil: no filter, every account.
enum FocusAccounts {
    private static let key = "focus.accounts"

    static var allowed: [String]? {
        get { SharedStorage.defaults.stringArray(forKey: key) }
        set {
            if let newValue, !newValue.isEmpty {
                SharedStorage.defaults.set(newValue.map { $0.lowercased() }, forKey: key)
            } else {
                SharedStorage.defaults.removeObject(forKey: key)
            }
        }
    }

    static func allows(_ email: String) -> Bool {
        allowed.map { $0.contains(email.lowercased()) } ?? true
    }
}
