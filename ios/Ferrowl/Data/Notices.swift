import Foundation
import Observation

/// Short confirmations and errors shown at the bottom of every screen
/// ("Archived · Undo", "Message sent"), one at a time. Confirmations go
/// away by themselves; errors stay until closed.
@MainActor
@Observable
final class Notices {
    struct Notice: Identifiable, Equatable {
        let id = UUID()
        let text: String
        let isError: Bool
        let actionLabel: String?
        let action: (() -> Void)?

        static func == (a: Notice, b: Notice) -> Bool { a.id == b.id }
    }

    private(set) var current: Notice?
    private var hide: Task<Void, Never>?

    /// How long a notice stays (also the time to press Undo).
    static let duration: Duration = .seconds(6)

    func info(_ text: String) { show(Notice(text: text, isError: false, actionLabel: nil, action: nil)) }

    func error(_ text: String) { show(Notice(text: text, isError: true, actionLabel: nil, action: nil)) }

    func withAction(_ text: String, _ label: String, _ action: @escaping () -> Void) {
        show(Notice(text: text, isError: false, actionLabel: label, action: action))
    }

    func dismiss() {
        hide?.cancel()
        current = nil
    }

    /// The notice's button was pressed.
    func act() {
        let action = current?.action
        dismiss()
        action?()
    }

    private func show(_ notice: Notice) {
        guard !notice.text.isEmpty else { return }
        hide?.cancel()
        current = notice
        // Errors stay until closed: they often explain what to do.
        if notice.isError { return }
        hide = Task { [weak self] in
            try? await Task.sleep(for: Self.duration)
            guard !Task.isCancelled else { return }
            if self?.current == notice { self?.current = nil }
        }
    }
}
