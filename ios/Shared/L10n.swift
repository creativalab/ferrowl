import Foundation

/// UI texts come from the engine (`core/locales/*.toml`, the same files as
/// the desktop and Android apps), so a translation is written once for all.
///
/// `t("selected_count", ["n": "3"])` fills the `{n}` placeholder.
func t(_ key: String, _ args: [String: String] = [:]) -> String {
    var text = Translations.text(key)
    for (name, value) in args {
        text = text.replacingOccurrences(of: "{\(name)}", with: value)
    }
    return text
}

/// The engine's texts, each asked once: lists draw many rows, each with
/// several labels, and a call into the engine for each was too slow.
private enum Translations {
    private static let lock = NSLock()
    nonisolated(unsafe) private static var cache: [String: String] = [:]

    static func text(_ key: String) -> String {
        lock.lock()
        defer { lock.unlock() }
        if let text = cache[key] { return text }
        let text = translate(key: key)
        cache[key] = text
        return text
    }
}

/// An action was asked on one account but the app has moved to another
/// meanwhile: it is not done (it would hit the other account's messages
/// with the same numbers). Nothing to tell the user: that screen is gone.
struct AccountChanged: Error {}

/// The message of an engine failure (already in words for the user; empty
/// for nothing to show, which notices skip).
func describe(_ error: Error) -> String {
    if error is AccountChanged { return "" }
    if let e = error as? FerrowlError, case .Failed(let message) = e {
        return message
    }
    return t("err_generic")
}

enum Language {
    /// Languages of `core/locales`.
    static let supported = ["en", "it", "fr", "de", "es", "pt", "zh", "ja", "ko", "ar"]

    /// The phone's first preferred language Ferrowl has, English otherwise.
    static let code: String = {
        for preferred in Locale.preferredLanguages {
            let code = String(preferred.prefix(while: { $0 != "-" && $0 != "_" })).lowercased()
            if supported.contains(code) { return code }
        }
        return "en"
    }()

    /// Locale for dates, matching the language of the texts.
    static let locale: Locale = {
        let preferred = Locale.preferredLanguages.first ?? "en"
        return preferred.hasPrefix(code) ? Locale(identifier: preferred) : Locale(identifier: code)
    }()
}

enum Dates {
    /// Time of a list row: the time today, "Yesterday", the day this year,
    /// the full date before.
    static func short(_ timestamp: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        let cal = Calendar.current
        if cal.isDateInToday(date) {
            return date.formatted(Date.FormatStyle(time: .shortened).locale(Language.locale))
        }
        if cal.isDateInYesterday(date) { return t("short_yesterday") }
        if cal.isDate(date, equalTo: Date(), toGranularity: .year) {
            return date.formatted(Date.FormatStyle().month(.abbreviated).day().locale(Language.locale))
        }
        return date.formatted(Date.FormatStyle(date: .numeric).locale(Language.locale))
    }

    /// Full date and time, for the message header.
    static func full(_ timestamp: Int64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return date.formatted(Date.FormatStyle(date: .long, time: .shortened).locale(Language.locale))
    }

    static func day(_ date: Date) -> String {
        date.formatted(Date.FormatStyle().weekday(.wide).day().month(.wide).locale(Language.locale))
    }

    static func time(_ timestamp: Int64) -> String {
        Date(timeIntervalSince1970: TimeInterval(timestamp))
            .formatted(Date.FormatStyle(time: .shortened).locale(Language.locale))
    }
}
