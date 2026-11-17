#if DEBUG
import Foundation
import SwiftUI

/// Development builds only: made-up mail for the App Store screenshots, in
/// the app's language. Started from the command line:
///
///     xcrun devicectl device process launch --terminate-existing \
///       --environment-variables '{"FERROWL_DEMO":"inbox"}' \
///       --device <UDID> com.ferrowl.app -AppleLanguages '(de)' -AppleLocale de_DE
///
/// Scenes: `inbox`, `message` (a newsletter with its images blocked),
/// `undo`, `accounts` (the sidebar), `pgp` (a signed and encrypted
/// message), `calendar`, `invite` (a message with an invitation).
///
/// Nothing real is read or written: the accounts, folders, messages and
/// events below live in memory only; the engine works in an empty temporary
/// folder and is never connected; every server action is refused quietly
/// (`MailRepository.ready`), and the settings, Keychain, accounts file,
/// badge, Spotlight, widget, push server, App Store and background checks
/// are left alone.
enum Demo {
    enum Scene: String { case inbox, message, undo, accounts, pgp, calendar, invite }

    static let isOn = ProcessInfo.processInfo.environment["FERROWL_DEMO"] != nil
    static let scene = Scene(rawValue: ProcessInfo.processInfo.environment["FERROWL_DEMO"] ?? "") ?? .inbox

    /// The engine's folder in demo mode (stays empty: it is never connected).
    static let directory = FileManager.default.temporaryDirectory.appendingPathComponent("demo", isDirectory: true)

    /// A sample text in the app's language.
    static func text(_ key: String) -> String {
        texts[Language.code]?[key] ?? texts["en"]?[key] ?? key
    }

    // MARK: accounts

    static let accounts: [Account] = [
        account("Alex Morgan", "alex.morgan@example.com"),
        account("Alex Morgan", "alex@example.org"),
        account("Alex M.", "alex.m@example.net"),
    ]

    private static func account(_ name: String, _ email: String) -> Account {
        Account(displayName: name, email: email, username: email, password: "", incomingHost: "imap.example.com",
                incomingPort: 993, smtpHost: "smtp.example.com", smtpPort: 465, protocol: .imap,
                pop3KeepDays: nil, pgpPassphrase: "")
    }

    /// Unread mail in the inbox of the accounts not in use (the sidebar).
    static func inboxUnread(_ email: String) -> Int {
        switch email {
        case accounts[1].email: 7
        case accounts[2].email: 2
        default: inbox.filter { !$0.seen }.count
        }
    }

    // MARK: folders

    static var folders: [Folder] {
        func folder(_ id: String, _ name: String, _ kind: FolderKind) -> Folder {
            Folder(id: id, name: name, path: [name], kind: kind, selectable: true)
        }
        return [
            folder("INBOX", t("folder_inbox"), .inbox),
            folder("Drafts", t("folder_drafts"), .drafts),
            folder("Sent", t("folder_sent"), .sent),
            folder("Archive", t("folder_archive"), .archive),
            folder("Junk", t("folder_spam"), .spam),
            folder("Trash", t("folder_trash"), .trash),
            folder("Projects", text("f_projects"), .other),
            folder("Travel", text("f_travel"), .other),
        ]
    }

    static var unread: [String: UInt32] {
        ["INBOX": UInt32(inbox.filter { !$0.seen }.count), "Junk": 3, "Projects": 2]
    }

    // MARK: messages

    private static let now = Int64(Date().timeIntervalSince1970)

    static let newsletter: UInt32 = 113
    static let signed: UInt32 = 112
    static let invitation: UInt32 = 111
    /// The message the `undo` scene moves to the Trash.
    static let trashed: UInt32 = 109

    static func headers(_ folderId: String) -> [Header] {
        folderId == "INBOX" ? inbox : []
    }

    static var inbox: [Header] {
        func header(_ uid: UInt32, _ from: String, _ address: String, _ subject: String, _ preview: String,
                    minutesAgo: Int64, seen: Bool, attachment: Bool = false, flagged: Bool = false,
                    id: String = "", parents: [String] = []) -> Header {
            Header(uid: uid, from: from, subject: subject, preview: preview, timestamp: now - minutesAgo * 60, seen: seen,
                   hasAttachment: attachment, messageId: id.isEmpty ? "demo-\(uid)@example.com" : id, parents: parents,
                   flagged: flagged, size: 24_000, fromAddress: address)
        }
        let hike = text("hike_subject")
        return [
            header(114, "Sofia Marino", "sofia.marino@example.com", "Re: \(hike)", text("hike_3"), minutesAgo: 12, seen: false,
                   id: "hike-3@example.com", parents: ["hike-2@example.com", "hike-1@example.com"]),
            header(newsletter, text("news_from"), "hello@news.example.org", text("news_subject"), text("news_preview"),
                   minutesAgo: 38, seen: false),
            header(signed, "Daniel Weber", "daniel.weber@example.org", text("pgp_subject"), text("pgp_preview"),
                   minutesAgo: 95, seen: true, attachment: true, flagged: true),
            header(invitation, "Yuki Tanaka", "yuki.tanaka@example.org", text("inv_subject"), text("inv_preview"),
                   minutesAgo: 170, seen: false),
            header(110, "Marco Rossi", "marco.rossi@example.net", "Re: \(hike)", text("hike_2"), minutesAgo: 200, seen: true,
                   id: "hike-2@example.com", parents: ["hike-1@example.com"]),
            header(trashed, text("lib_from"), "desk@library.example.org", text("lib_subject"), text("lib_preview"),
                   minutesAgo: 300, seen: true),
            header(108, "Sofia Marino", "sofia.marino@example.com", hike, text("hike_1"), minutesAgo: 420, seen: true,
                   id: "hike-1@example.com"),
            header(107, "Lucas Ferreira", "lucas.ferreira@example.com", text("photos_subject"), text("photos_preview"),
                   minutesAgo: 26 * 60, seen: true, attachment: true, flagged: true),
            header(106, "Amira Haddad", "amira.haddad@example.org", text("budget_subject"), text("budget_preview"),
                   minutesAgo: 30 * 60, seen: false),
            header(105, "Clara Dubois", "clara.dubois@example.net", text("dinner_subject"), text("dinner_preview"),
                   minutesAgo: 50 * 60, seen: true),
            header(104, "Min-jun Park", "minjun.park@example.org", text("slides_subject"), text("slides_preview"),
                   minutesAgo: 73 * 60, seen: true, attachment: true),
            header(103, text("coop_from"), "orders@market.example.com", text("coop_subject"), text("coop_preview"),
                   minutesAgo: 98 * 60, seen: true),
        ]
    }

    static func search(_ folderId: String, _ query: String) -> [Header] {
        headers(folderId).filter {
            $0.subject.localizedCaseInsensitiveContains(query) || $0.from.localizedCaseInsensitiveContains(query)
                || $0.preview.localizedCaseInsensitiveContains(query)
        }
    }

    static func message(_ uid: UInt32) throws -> Message {
        guard let h = inbox.first(where: { $0.uid == uid }) else { throw AccountChanged() }
        let me = "Alex Morgan"
        var message = Message(uid: uid, from: h.from, fromAddress: h.fromAddress, to: me, cc: "",
                              toAddresses: [accounts[0].email], ccAddresses: [], replyToAddresses: [],
                              subject: h.subject, timestamp: h.timestamp, messageId: h.messageId, references: h.parents,
                              text: h.preview, html: nil, inline: [:], attachments: [], dkimPass: true, pgp: nil,
                              calendar: nil, invitation: nil)
        switch uid {
        case newsletter:
            message.to = accounts[0].email
            message.html = newsletterHTML
            message.text = [text("news_title"), text("news_intro")].joined(separator: "\n\n")
        case signed:
            message.text = text("pgp_body")
            message.pgp = PgpInfo(encrypted: true, signature: .valid, error: nil)
            message.attachments = [Attachment(name: "contract-v3.pdf", data: Data(count: 248_312))]
        case invitation:
            message.text = text("inv_body")
            let start = day(2, hour: 10)
            message.invitation = Invitation(method: "REQUEST", uid: "design-review@example.org", summary: text("inv_summary"),
                                            location: text("inv_location"), start: start, end: start + 3600, allDay: false,
                                            firstDay: nil, lastDay: nil, recurring: false, organizer: h.fromAddress,
                                            organizerName: h.from, answers: [])
        default:
            break
        }
        return message
    }

    /// A newsletter with remote images (blocked, so the bar to load them shows).
    private static var newsletterHTML: String {
        let dir = Language.code == "ar" ? "rtl" : "ltr"
        func item(_ n: Int) -> String {
            """
            <tr><td class="num">\(n)</td><td class="it"><b>\(text("news_\(n)_title"))</b><br>\(text("news_\(n)"))</td></tr>
            """
        }
        return """
        <!doctype html><html dir="\(dir)"><head><style>
        body { background: #f4efe8; }
        .wrap { max-width: 600px; margin: 0 auto; background: #fffdf9; color: #2b2622; font-family: -apple-system, sans-serif; }
        .mast { background: #2f4a3c; color: #f5efe6; padding: 20px 22px; }
        .brand { font-size: 22px; font-weight: 800; letter-spacing: 3px; }
        .issue { font-size: 13px; opacity: .75; margin-top: 4px; }
        .hero { height: 150px; background: linear-gradient(135deg, #eadfcf, #d6c2a6); }
        .body { padding: 20px 22px 8px; }
        h1 { font-size: 24px; line-height: 1.25; margin: 0 0 10px; color: #2f4a3c; }
        p { font-size: 16px; line-height: 1.5; margin: 0 0 14px; }
        table { border-collapse: collapse; width: 100%; }
        td { padding: 10px 0; vertical-align: top; font-size: 15px; line-height: 1.45; border-top: 1px solid #eee4d8; }
        .num { width: 40px; font-size: 22px; font-weight: 800; color: #c0622b; }
        .btn { display: inline-block; margin: 18px 0 22px; padding: 12px 22px; border-radius: 24px; background: #c0622b;
               color: #fff; text-decoration: none; font-weight: 700; }
        .foot { padding: 16px 22px 24px; font-size: 12px; color: #8a8178; background: #f4efe8; }
        </style></head><body><div class="wrap">
        <div class="mast"><div class="brand">FIELD NOTES</div><div class="issue">№ 48</div></div>
        <div class="hero"><img src="https://news.example.org/img/48/hero.jpg" alt=""></div>
        <div class="body"><h1>\(text("news_title"))</h1><p>\(text("news_intro"))</p>
        <table>\(item(1))\(item(2))\(item(3))</table>
        <a class="btn" href="https://news.example.org/48">\(text("news_button"))</a></div>
        <div class="foot">\(text("news_footer"))</div>
        <img src="https://news.example.org/open/48.gif" width="1" height="1" alt="">
        </div></body></html>
        """
    }

    // MARK: calendar

    /// `days` from today at `hour` (local time), in seconds.
    private static func day(_ days: Int, hour: Int, minute: Int = 0) -> Int64 {
        let cal = Calendar.current
        let start = cal.date(byAdding: .day, value: days, to: cal.startOfDay(for: Date()))!
        let date = cal.date(bySettingHour: hour, minute: minute, second: 0, of: start)!
        return Int64(date.timeIntervalSince1970)
    }

    static func agenda(from: Int64, to: Int64) -> [CalendarEvent] {
        func event(_ id: String, _ summary: String, _ location: String, _ start: Int64, minutes: Int64,
                   color: String, recurring: Bool = false) -> CalendarEvent {
            CalendarEvent(uid: id, summary: summary, location: location, description: "", start: start,
                          end: start + minutes * 60, allDay: false, firstDay: nil, lastDay: nil, recurring: recurring,
                          organizer: "", calendar: "", color: color, url: "demo:\(id)", calendarUrl: "demo:")
        }
        let work = "#1E5AA8", personal = "#A63A0C", sport = "#2F6B45"
        let birthday = Date(timeIntervalSince1970: TimeInterval(day(4, hour: 12)))
        let events = [
            event("standup", text("ev_standup"), text("ev_call"), day(0, hour: 9, minute: 30), minutes: 15, color: work,
                  recurring: true),
            event("lunch", text("ev_lunch"), text("ev_lunch_loc"), day(0, hour: 13), minutes: 60, color: personal),
            event("yoga", text("ev_yoga"), "", day(1, hour: 18, minute: 30), minutes: 60, color: sport, recurring: true),
            event("review", text("inv_summary"), text("inv_location"), day(2, hour: 10), minutes: 60, color: work),
            event("standup2", text("ev_standup"), text("ev_call"), day(2, hour: 9, minute: 30), minutes: 15, color: work,
                  recurring: true),
            event("dentist", text("ev_dentist"), "", day(3, hour: 8, minute: 30), minutes: 30, color: personal),
            CalendarEvent(uid: "birthday", summary: text("ev_birthday"), location: "", description: "",
                          start: day(4, hour: 0), end: day(5, hour: 0), allDay: true,
                          firstDay: CalendarView.key(birthday), lastDay: CalendarView.key(birthday), recurring: true,
                          organizer: "", calendar: "", color: personal, url: "demo:birthday", calendarUrl: "demo:"),
            event("hike", text("ev_hike"), text("ev_hike_loc"), day(5, hour: 8), minutes: 360, color: sport),
        ]
        return events.filter { $0.start < to && $0.end > from }.sorted { $0.start < $1.start }
    }

    // MARK: scenes

    /// Puts the mailbox in the state of the scene asked for, once it has started.
    @MainActor
    static func stage(_ model: MailboxModel, column: Binding<NavigationSplitViewColumn>,
                      sheet: Binding<MainView.Sheet?>) async {
        try? await Task.sleep(for: .milliseconds(400))
        func open(_ uid: UInt32) {
            if let row = model.rows.first(where: { $0.id == uid }) { model.open = row }
        }
        switch scene {
        case .inbox: break
        case .message: open(newsletter)
        case .pgp: open(signed)
        case .invite: open(invitation)
        case .accounts: column.wrappedValue = .sidebar
        case .calendar: sheet.wrappedValue = .calendar
        case .undo:
            if let row = model.rows.first(where: { $0.id == trashed }) { model.deleteWithUndo(row) }
        }
    }
}
#endif
