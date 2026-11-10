import AppIntents
import Foundation

/// The widget's buttons on a message: mark it read, archive it. They run in
/// the app's process (a `LiveActivityIntent` does, even with the app
/// closed), where the app sets `WidgetActions.run`; the widget has no
/// engine of its own.
enum WidgetActions {
    /// (message link, archive instead of marking read). Set by the app.
    @MainActor static var run: ((String, Bool) async throws -> Void)?

    /// The app was not there to do it.
    struct Unavailable: Error {}

    @MainActor
    static func perform(_ link: String, archive: Bool) async throws {
        guard let run else { throw Unavailable() }
        try await run(link, archive)
    }
}

struct WidgetMarkReadIntent: LiveActivityIntent {
    /// The widget's own button, not an action offered in Shortcuts
    /// (Shortcuts has "Archive mail" and "Mark mail", with Pro and a
    /// confirmation).
    static var isDiscoverable: Bool { false }
    static var title: LocalizedStringResource = "intent_widget_read"
    static var openAppWhenRun = false

    @Parameter(title: "intent_link")
    var link: String

    init() {}

    init(link: String) {
        self.link = link
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        try await WidgetActions.perform(link, archive: false)
        return .result()
    }
}

struct WidgetArchiveIntent: LiveActivityIntent {
    /// The widget's own button, not an action offered in Shortcuts
    /// (Shortcuts has "Archive mail" and "Mark mail", with Pro and a
    /// confirmation).
    static var isDiscoverable: Bool { false }
    static var title: LocalizedStringResource = "intent_widget_archive"
    static var openAppWhenRun = false

    @Parameter(title: "intent_link")
    var link: String

    init() {}

    init(link: String) {
        self.link = link
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        try await WidgetActions.perform(link, archive: true)
        return .result()
    }
}
