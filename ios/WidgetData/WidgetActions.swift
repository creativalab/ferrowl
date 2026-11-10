import AppIntents
import Foundation

/// The widget's buttons on a message: mark it read, archive it. They run in
/// the app's process (a `LiveActivityIntent` does, even with the app
/// closed), where the app sets `WidgetActions.run`; the widget has no
/// engine of its own.
enum WidgetActions {
    /// (message link, archive instead of marking read). Set by the app.
    @MainActor static var run: ((String, Bool) async throws -> Void)?
}

struct WidgetMarkReadIntent: LiveActivityIntent {
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
        try await WidgetActions.run?(link, false)
        return .result()
    }
}

struct WidgetArchiveIntent: LiveActivityIntent {
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
        try await WidgetActions.run?(link, true)
        return .result()
    }
}
