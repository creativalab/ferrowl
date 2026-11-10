import SwiftUI
import UIKit

/// What a message is about, picked out of its text by the engine
/// (`mailInsights`): a shipment to follow, a code to copy, a bill whose due
/// date can go in the calendar. Shown above the message, like an invitation.
struct InsightCards: View {
    let insights: [MailInsight]

    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.openURL) private var openURL

    var body: some View {
        ForEach(Array(insights.enumerated()), id: \.offset) { _, insight in
            card(insight)
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 14))
        }
    }

    @ViewBuilder
    private func card(_ insight: MailInsight) -> some View {
        switch insight {
        case .shipment(let carrier, let number, let url):
            VStack(alignment: .leading, spacing: 8) {
                Label(carrier.isEmpty ? t("insight_shipment") : "\(t("insight_shipment")) · \(carrier)",
                      systemImage: "shippingbox")
                    .font(.subheadline.weight(.semibold)).foregroundStyle(.tint)
                Text(number).font(.body.monospaced()).textSelection(.enabled)
                HStack {
                    if let link = URL(string: url), !url.isEmpty {
                        Button(t("insight_track")) { openURL(link) }.buttonStyle(.borderedProminent)
                    }
                    Button(t("insight_copy")) { copy(number) }.buttonStyle(.bordered)
                }
            }
        case .code(let code):
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Label(t("insight_code"), systemImage: "key")
                        .font(.subheadline.weight(.semibold)).foregroundStyle(.tint)
                    Text(code).font(.title2.monospaced().weight(.semibold)).textSelection(.enabled)
                }
                Spacer()
                Button(t("insight_copy")) { copy(code) }.buttonStyle(.borderedProminent)
            }
        case .payment(let number, let amount, let due):
            VStack(alignment: .leading, spacing: 8) {
                Label(number.isEmpty ? t("insight_bill") : "\(t("insight_bill")) · \(number)", systemImage: "doc.text")
                    .font(.subheadline.weight(.semibold)).foregroundStyle(.tint)
                if !amount.isEmpty { Text(amount).font(.title3.weight(.semibold)) }
                if let date = Self.date(due) {
                    Text(t("insight_due", ["date": date.formatted(Date.FormatStyle(date: .long).locale(Language.locale))]))
                        .font(.subheadline)
                    Button(t("insight_add_due")) { addDue(due, number: number, amount: amount) }
                        .buttonStyle(.bordered)
                }
            }
        }
    }

    private static func date(_ text: String) -> Date? {
        guard !text.isEmpty else { return nil }
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyy-MM-dd"
        return format.date(from: text)
    }

    private func copy(_ text: String) {
        UIPasteboard.general.string = text
        notices.info(t("insight_copied"))
    }

    /// An all-day reminder on the due date, in the account's first calendar
    /// (the one on the phone when it has no calendar server).
    private func addDue(_ due: String, number: String, amount: String) {
        Task {
            guard let calendar = await mail.calendars().first else {
                notices.error(t("err_no_calendar"))
                return
            }
            let what = [number, amount].filter { !$0.isEmpty }.joined(separator: " · ")
            let draft = EventDraft(summary: t("insight_due_title", ["what": what.isEmpty ? t("insight_bill") : what]),
                                   location: "", description: "", allDay: true,
                                   start: "\(due)T09:00", end: "\(due)T10:00", repeat: .never)
            do {
                _ = try await mail.saveEvent(calendarUrl: calendar.url, url: nil, draft: draft)
                notices.info(t("insight_due_added"))
            } catch {
                notices.error(describe(error))
            }
        }
    }
}
