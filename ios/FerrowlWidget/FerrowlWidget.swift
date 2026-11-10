import SwiftUI
import WidgetKit

// Ferrowl's widget: unread mail on the Home Screen and the Lock Screen.
// It reads what the app and the notification extension leave in the App
// Group (`WidgetData`), which also reload it; texts are keys of the
// widget's Localizable.strings, generated from core/locales.

@main
struct FerrowlWidgets: WidgetBundle {
    var body: some Widget {
        UnreadWidget()
    }
}

struct UnreadEntry: TimelineEntry {
    let date: Date
    let accounts: [WidgetAccount]

    var total: Int { accounts.reduce(0) { $0 + $1.unread } }
    /// The newest unread messages of every account.
    var recent: [WidgetMail] {
        Array(accounts.flatMap(\.recent).sorted { $0.date > $1.date }.prefix(WidgetData.recentLimit))
    }

    static let sample = UnreadEntry(date: Date(), accounts: [
        WidgetAccount(email: "anna@example.com", unread: 3, recent: [
            WidgetMail(from: "Giulia Ferri", subject: "Riunione di domani", date: Date(), uid: 2, link: "ferrowl://inbox"),
            WidgetMail(from: "Marco Bianchi", subject: "Fattura ottobre", date: Date().addingTimeInterval(-3600), uid: 1,
                       link: "ferrowl://inbox"),
        ]),
    ])
}

struct UnreadProvider: TimelineProvider {
    func placeholder(in context: Context) -> UnreadEntry { .sample }

    func getSnapshot(in context: Context, completion: @escaping (UnreadEntry) -> Void) {
        let accounts = WidgetData.load()
        completion(context.isPreview && accounts.isEmpty ? .sample : UnreadEntry(date: Date(), accounts: accounts))
    }

    /// One entry: the app reloads the widget whenever the numbers change.
    func getTimeline(in context: Context, completion: @escaping (Timeline<UnreadEntry>) -> Void) {
        completion(Timeline(entries: [UnreadEntry(date: Date(), accounts: WidgetData.load())], policy: .never))
    }
}

struct UnreadWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: WidgetData.kind, provider: UnreadProvider()) { entry in
            UnreadView(entry: entry)
                .containerBackground(.fill.tertiary, for: .widget)
                .widgetURL(URL(string: "ferrowl://inbox"))
        }
        .configurationDisplayName(LocalizedStringResource("widget_title"))
        .description(LocalizedStringResource("widget_desc"))
        .supportedFamilies([.systemSmall, .systemMedium, .accessoryCircular, .accessoryRectangular, .accessoryInline])
    }
}

private let accent = Color(red: 0.88, green: 0.48, blue: 0.29)

private func text(_ key: String, _ n: Int? = nil) -> String {
    let s = Bundle.main.localizedString(forKey: key, value: nil, table: nil)
    return n.map { s.replacingOccurrences(of: "{n}", with: "\($0)") } ?? s
}

struct UnreadView: View {
    let entry: UnreadEntry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        switch family {
        case .accessoryCircular:
            ZStack {
                AccessoryWidgetBackground()
                VStack(spacing: 0) {
                    Image(systemName: "envelope.fill").font(.caption2)
                    Text("\(entry.total)").font(.title3.bold()).minimumScaleFactor(0.5)
                }
            }
        case .accessoryInline:
            Label(entry.total == 0 ? text("widget_none") : text("widget_inline", entry.total), systemImage: "envelope")
        case .accessoryRectangular:
            VStack(alignment: .leading, spacing: 1) {
                Label(entry.total == 0 ? text("widget_none") : text("widget_inline", entry.total), systemImage: "envelope")
                    .font(.headline)
                if let mail = entry.recent.first {
                    Text(mail.from).font(.caption).lineLimit(1)
                    Text(mail.subject).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
        case .systemMedium:
            HStack(alignment: .top, spacing: 14) {
                count
                VStack(alignment: .leading, spacing: 6) {
                    if entry.recent.isEmpty {
                        Spacer()
                        Text(text("widget_none")).font(.subheadline).foregroundStyle(.secondary)
                        Spacer()
                    }
                    ForEach(entry.recent.prefix(3), id: \.self) { mail in
                        HStack(spacing: 6) {
                            Link(destination: URL(string: mail.link) ?? URL(string: "ferrowl://inbox")!) {
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(mail.from).font(.subheadline.weight(.semibold)).lineLimit(1)
                                    Text(mail.subject).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            // Without opening the app.
                            Button(intent: WidgetMarkReadIntent(link: mail.link)) {
                                Image(systemName: "envelope.open")
                            }
                            .buttonStyle(.plain).foregroundStyle(accent)
                            Button(intent: WidgetArchiveIntent(link: mail.link)) {
                                Image(systemName: "archivebox")
                            }
                            .buttonStyle(.plain).foregroundStyle(accent)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        default:
            VStack(alignment: .leading, spacing: 4) {
                count
                Spacer(minLength: 0)
                if let mail = entry.recent.first {
                    Text(mail.from).font(.caption.weight(.semibold)).lineLimit(1)
                    Text(mail.subject).font(.caption2).foregroundStyle(.secondary).lineLimit(2)
                } else if entry.total == 0 {
                    Text(text("widget_none")).font(.caption).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// The total, big, with "unread" under it.
    private var count: some View {
        VStack(alignment: .leading, spacing: 0) {
            Image(systemName: "envelope.fill").foregroundStyle(accent).font(.title3)
            Text("\(entry.total)").font(.system(size: 40, weight: .bold, design: .rounded)).minimumScaleFactor(0.5)
            Text(text("widget_unread")).font(.caption).foregroundStyle(.secondary)
        }
    }
}
