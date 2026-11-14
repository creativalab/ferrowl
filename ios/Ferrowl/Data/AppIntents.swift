import AppIntents
import Foundation

// Ferrowl's actions for Shortcuts, Siri and Spotlight (Ferrowl Bridge on
// iOS, see docs/integrazioni-desktop.md). They run in the app's process:
// reading works from what the phone keeps and from the server; sending
// always asks the user first and needs Ferrowl Pro; writing opens the app
// with the draft, never sends it.
//
// Titles are keys of Localizable.strings and Siri's phrases are keys of
// AppShortcuts.strings, both generated from core/locales by
// ffi/build-ios.sh.

/// Failures shown by Shortcuts/Siri, already in words for the user.
struct BridgeError: Error, CustomLocalizedStringResourceConvertible {
    let message: String
    var localizedStringResource: LocalizedStringResource { "\(message)" }
}

@MainActor
enum Bridge {
    static func app() throws -> AppModel {
        guard let app = AppModel.shared else { throw BridgeError(message: t("err_generic")) }
        return app
    }

    /// The accounts an action works on: the one asked for, or every account
    /// Ferrowl may use (the first one without Pro).
    static func accounts(_ wanted: AccountEntity?) async throws -> [Account] {
        let app = try app()
        // A shortcut can start the app in the background, before it asked
        // the App Store: Ferrowl Pro is known first.
        await app.pro.ready()
        let usable = app.mail.accounts.filter { app.mayUse($0.email) }
        guard !usable.isEmpty else { throw BridgeError(message: t("intent_no_account")) }
        guard let wanted else { return usable }
        let chosen = usable.filter { $0.email.caseInsensitiveCompare(wanted.id) == .orderedSame }
        guard !chosen.isEmpty else { throw BridgeError(message: t("intent_no_account")) }
        return chosen
    }

    /// The account to send from: the one asked for, or the one in use.
    static func sender(_ wanted: AccountEntity?) async throws -> Account {
        let app = try app()
        let usable = try await accounts(wanted)
        if wanted == nil, let active = app.mail.activeAccount, usable.contains(where: { $0.email == active.email }) {
            return active
        }
        return usable[0]
    }

    /// Messages handed out recently, so a shortcut can pass one to "Open
    /// mail" with its details.
    private static var recent: [String: MailEntity] = [:]

    static func remember(_ items: [MailEntity]) {
        if recent.count > 500 { recent.removeAll() }
        for item in items { recent[item.id] = item }
    }

    static func mail(_ id: String) -> MailEntity? {
        if let known = recent[id] { return known }
        guard case .open(let account, let folder, let uid) = parseLink(url: id) else { return nil }
        return MailEntity(account: account, folder: folder, uid: uid)
    }

    /// "3 unread messages. Anna: Invoice; Marco: Dinner…" — the count and
    /// the first few, as Siri reads them.
    static func summary(_ count: String, _ items: [MailEntity]) -> String {
        let firsts = items.prefix(3).map { "\($0.sender): \($0.subject)" }
        return ([count] + firsts).joined(separator: "\n")
    }
}

// MARK: entities

struct AccountEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "intent_account")
    static var defaultQuery = AccountQuery()

    /// The address.
    let id: String
    let name: String

    var displayRepresentation: DisplayRepresentation {
        name.isEmpty || name == id
            ? DisplayRepresentation(title: "\(id)")
            : DisplayRepresentation(title: "\(name)", subtitle: "\(id)")
    }

    init(_ account: Account) {
        id = account.email
        name = account.displayName
    }
}

struct AccountQuery: EntityQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [AccountEntity] {
        guard let app = AppModel.shared else { return [] }
        return app.mail.accounts
            .filter { a in identifiers.contains { $0.caseInsensitiveCompare(a.email) == .orderedSame } }
            .map(AccountEntity.init)
    }

    @MainActor
    func suggestedEntities() async throws -> [AccountEntity] {
        guard let app = AppModel.shared else { return [] }
        await app.pro.ready()
        return app.mail.accounts.filter { app.mayUse($0.email) }.map(AccountEntity.init)
    }
}

/// A message, identified by its `ferrowl://open` link (which other apps
/// can keep and open later).
struct MailEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "intent_message")
    static var defaultQuery = MailQuery()

    let id: String
    let account: String
    let folder: String
    let uid: UInt32

    @Property(title: "intent_from") var sender: String
    @Property(title: "intent_subject") var subject: String
    @Property(title: "intent_date") var date: Date
    @Property(title: "intent_preview") var preview: String
    @Property(title: "intent_unread") var unread: Bool
    @Property(title: "intent_account") var address: String
    @Property(title: "intent_link") var link: URL

    init(account: String, folder: String, uid: UInt32, header: Header? = nil) {
        let link = messageLink(account: account, folder: folder, uid: uid)
        id = link
        self.account = account
        self.folder = folder
        self.uid = uid
        sender = header?.from ?? ""
        subject = header?.subject ?? ""
        date = Date(timeIntervalSince1970: TimeInterval(header?.timestamp ?? 0))
        preview = header?.preview ?? ""
        unread = !(header?.seen ?? true)
        address = account
        self.link = URL(string: link) ?? URL(string: "ferrowl://inbox")!
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(
            title: "\(subject.isEmpty ? t("no_subject") : subject)",
            subtitle: "\(sender)"
        )
    }
}

struct MailQuery: EntityQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [MailEntity] {
        identifiers.compactMap(Bridge.mail)
    }
}

// MARK: actions

struct UnreadMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_unread_title"
    static var description = IntentDescription("intent_unread_desc")

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<[MailEntity]> & ProvidesDialog {
        let app = try Bridge.app()
        var found: [MailEntity] = []
        var total: UInt32 = 0
        for a in try await Bridge.accounts(account) {
            // From the server (on a connection of its own, the app's stays
            // as it is); POP3 accounts and failures: what the phone keeps.
            if let peek = try? await app.engine.peekInbox(account: a, after: 0, limit: 25) {
                total += peek.freshCount
                found += peek.fresh.map { MailEntity(account: a.email, folder: "INBOX", uid: $0.uid, header: $0) }
            } else {
                let engine = app.engine
                let cached = await Task.detached { engine.cachedInbox(account: a) }.value.filter { !$0.seen }
                total += UInt32(cached.count)
                found += cached.prefix(25).map { MailEntity(account: a.email, folder: "INBOX", uid: $0.uid, header: $0) }
            }
        }
        found.sort { $0.date > $1.date }
        Bridge.remember(found)
        let text = total == 0
            ? t("intent_no_unread")
            : Bridge.summary(t("intent_unread_count", ["n": "\(total)"]), found)
        return .result(value: found, dialog: "\(text)")
    }
}

struct SearchMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_search_title"
    static var description = IntentDescription("intent_search_desc")

    @Parameter(title: "intent_query", requestValueDialog: "intent_ask_query")
    var query: String

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    static var parameterSummary: some ParameterSummary {
        Summary("\(\.$query)") { \.$account }
    }

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<[MailEntity]> & ProvidesDialog {
        let app = try Bridge.app()
        let engine = app.engine
        let accounts = try await Bridge.accounts(account)
        let query = query
        // Inbox and Sent as stored on the phone (with the search syntax:
        // from:, has:attachment, is:unread…).
        let sent = app.mail.folder(of: .sent)?.id
        let active = app.mail.activeEmail
        let found: [MailEntity] = await Task.detached {
            accounts.flatMap { a in
                var folders = ["INBOX"]
                if let sent, a.email == active { folders.append(sent) }
                return folders.flatMap { folder in
                    engine.searchAccount(account: a, folderId: folder, query: query, limit: 25)
                        .map { MailEntity(account: a.email, folder: folder, uid: $0.uid, header: $0) }
                }
            }
        }.value
        let sorted = Array(found.sorted { $0.date > $1.date }.prefix(25))
        Bridge.remember(sorted)
        let text = sorted.isEmpty
            ? t("intent_none_found")
            : Bridge.summary(t("intent_found", ["n": "\(sorted.count)"]), sorted)
        return .result(value: sorted, dialog: "\(text)")
    }
}

struct SendMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_send_title"
    static var description = IntentDescription("intent_send_desc")

    @Parameter(title: "intent_to", requestValueDialog: "intent_ask_to")
    var to: String

    @Parameter(title: "intent_subject")
    var subject: String?

    @Parameter(title: "intent_body", inputOptions: String.IntentInputOptions(multiline: true))
    var body: String?

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    static var parameterSummary: some ParameterSummary {
        Summary("\(\.$to) \(\.$subject)") {
            \.$body
            \.$account
        }
    }

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let app = try Bridge.app()
        // Acting for the user is Ferrowl Pro (also when the app was just
        // started for this: ask the App Store first).
        if capabilityNeedsPro(capability: .send), !app.pro.pro { await app.pro.refresh() }
        guard !capabilityNeedsPro(capability: .send) || app.pro.pro else {
            throw BridgeError(message: t("intent_needs_pro"))
        }
        guard to.contains("@") else { throw $to.needsValueError("intent_ask_to") }
        let from = try await Bridge.sender(account)
        let subject = subject ?? ""
        let body = body ?? ""
        // Always confirmed: the text may come from another app, or from
        // an assistant that read it in a message.
        if capabilityNeedsConfirmation(capability: .send) {
            let question = t("intent_send_confirm", ["subject": subject.isEmpty ? t("no_subject") : subject, "to": to])
            try await requestConfirmation(result: .result(dialog: "\(question)"), confirmationActionName: .send)
        }
        let draft = Outgoing(to: to, cc: "", bcc: "", subject: subject, body: body,
                             html: markdownToHtml(markdown: body), inReplyTo: nil, references: [],
                             attachments: [], sign: false, encrypt: false)
        // An engine of its own: the app's may be busy, or on another account.
        let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
        do {
            _ = try await engine.connect(account: from)
            _ = try await engine.send(message: draft)
        } catch {
            throw BridgeError(message: describe(error))
        }
        app.mail.touch()
        return .result(dialog: "\(t("intent_sent"))")
    }
}

struct ComposeMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_compose_title"
    static var description = IntentDescription("intent_compose_desc")
    static var openAppWhenRun = true

    @Parameter(title: "intent_to")
    var to: String?

    @Parameter(title: "intent_subject")
    var subject: String?

    @Parameter(title: "intent_body", inputOptions: String.IntentInputOptions(multiline: true))
    var body: String?

    @Parameter(title: "intent_attachments")
    var files: [IntentFile]?

    @MainActor
    func perform() async throws -> some IntentResult {
        let app = try Bridge.app()
        // The files a shortcut passed, written where the composer can read them.
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("shared/\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let urls: [URL] = (files ?? []).compactMap { file in
            let url = folder.appendingPathComponent(file.filename.isEmpty ? "attachment" : file.filename)
            return (try? file.data.write(to: url)) != nil ? url : nil
        }
        app.pendingCompose = ComposeRequest(to: to ?? "", subject: subject ?? "", body: body ?? "", files: urls)
        return .result()
    }
}

struct OpenMailIntent: OpenIntent {
    static var title: LocalizedStringResource = "intent_open_title"

    @Parameter(title: "intent_message")
    var target: MailEntity

    @MainActor
    func perform() async throws -> some IntentResult {
        let app = try Bridge.app()
        app.open(link: target.link)
        return .result()
    }
}

// MARK: contacts and calendar

/// A person of the address book (CardDAV and the people mail was sent to).
struct ContactEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "intent_contact")
    static var defaultQuery = ContactQuery()

    /// The preferred address, or the name for contacts without one.
    let id: String

    @Property(title: "intent_name") var name: String
    @Property(title: "intent_emails") var emails: [String]
    @Property(title: "intent_phones") var phones: [String]
    @Property(title: "intent_organization") var organization: String

    init(_ contact: Contact) {
        id = contact.emails.first ?? contact.name
        name = contact.name
        emails = contact.emails
        phones = contact.phones
        organization = contact.organization
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(name.isEmpty ? id : name)", subtitle: "\(emails.first ?? phones.first ?? "")")
    }
}

struct ContactQuery: EntityQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [ContactEntity] {
        identifiers.compactMap { Bridge.contact($0) }
    }
}

/// One occurrence of a calendar event.
struct EventEntity: AppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "intent_event")
    static var defaultQuery = EventQuery()

    let id: String

    @Property(title: "intent_event_title") var title: String
    @Property(title: "intent_start") var start: Date
    @Property(title: "intent_end") var end: Date
    @Property(title: "intent_all_day") var allDay: Bool
    @Property(title: "intent_location") var location: String
    @Property(title: "intent_calendar") var calendar: String

    init(_ event: CalendarEvent) {
        id = "\(event.url)#\(event.start)"
        title = event.summary
        start = Date(timeIntervalSince1970: TimeInterval(event.start))
        end = Date(timeIntervalSince1970: TimeInterval(event.end))
        allDay = event.allDay
        location = event.location
        calendar = event.calendar
    }

    /// "9:00 Riunione" (or the title alone for all-day events).
    var spoken: String {
        allDay ? title : "\(Dates.time(Int64(start.timeIntervalSince1970))) \(title)"
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(title)", subtitle: "\(allDay ? calendar : Dates.time(Int64(start.timeIntervalSince1970)))")
    }
}

struct EventQuery: EntityQuery {
    @MainActor
    func entities(for identifiers: [String]) async throws -> [EventEntity] {
        identifiers.compactMap { Bridge.event($0) }
    }
}

extension Bridge {
    private static var contacts: [String: ContactEntity] = [:]
    private static var events: [String: EventEntity] = [:]

    static func remember(_ items: [ContactEntity]) {
        if contacts.count > 500 { contacts.removeAll() }
        for item in items { contacts[item.id] = item }
    }

    static func remember(_ items: [EventEntity]) {
        if events.count > 500 { events.removeAll() }
        for item in items { events[item.id] = item }
    }

    static func contact(_ id: String) -> ContactEntity? { contacts[id] }
    static func event(_ id: String) -> EventEntity? { events[id] }
}

struct AgendaIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_agenda_title"
    static var description = IntentDescription("intent_agenda_desc")

    @Parameter(title: "intent_day")
    var day: Date?

    @Parameter(title: "intent_days", default: 1, inclusiveRange: (1, 31))
    var days: Int

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<[EventEntity]> & ProvidesDialog {
        let app = try Bridge.app()
        let first = Calendar.current.startOfDay(for: day ?? Date())
        let last = Calendar.current.date(byAdding: .day, value: max(days, 1), to: first) ?? first
        let (from, to) = (Int64(first.timeIntervalSince1970), Int64(last.timeIntervalSince1970))
        let engine = app.engine
        let accounts = try await Bridge.accounts(account)
        // What was synced from the calendars (CalDAV), on the phone.
        let found: [EventEntity] = await Task.detached {
            accounts.flatMap { engine.agendaFor(account: $0, from: from, to: to) }.map(EventEntity.init)
        }.value.sorted { $0.start < $1.start }
        Bridge.remember(found)
        let text = found.isEmpty
            ? t("intent_no_events")
            : ([t("intent_events_count", ["n": "\(found.count)"])] + found.prefix(5).map(\.spoken)).joined(separator: "\n")
        return .result(value: found, dialog: "\(text)")
    }
}

struct FindContactIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_contact_title"
    static var description = IntentDescription("intent_contact_desc")

    @Parameter(title: "intent_name", requestValueDialog: "intent_ask_contact")
    var query: String

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<[ContactEntity]> & ProvidesDialog {
        let app = try Bridge.app()
        let engine = app.engine
        let accounts = try await Bridge.accounts(account)
        let query = query
        let found: [ContactEntity] = await Task.detached {
            var seen = Set<String>()
            return accounts.flatMap { engine.contactsFor(account: $0, query: query) }
                .map(ContactEntity.init)
                .filter { seen.insert($0.id.lowercased()).inserted }
        }.value
        let list = Array(found.prefix(25))
        Bridge.remember(list)
        let text: String
        if let first = list.first {
            text = [first.name.isEmpty ? first.id : first.name, first.emails.first, first.phones.first]
                .compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: "\n")
        } else {
            text = t("intent_no_contacts")
        }
        return .result(value: list, dialog: "\(text)")
    }
}

struct NewEventIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_new_event_title"
    static var description = IntentDescription("intent_new_event_desc")

    @Parameter(title: "intent_event_title", requestValueDialog: "intent_ask_event")
    var title: String

    @Parameter(title: "intent_start", requestValueDialog: "intent_ask_start")
    var start: Date

    @Parameter(title: "intent_duration", default: 60, inclusiveRange: (5, 1440))
    var minutes: Int

    @Parameter(title: "intent_location")
    var location: String?

    @Parameter(title: "intent_account")
    var account: AccountEntity?

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let app = try Bridge.app()
        let from = try await Bridge.sender(account)
        let engine = app.engine
        guard let calendar = await Task.detached(operation: { engine.calendarsFor(account: from) }).value.first else {
            throw BridgeError(message: t("err_no_calendar"))
        }
        // Adding to the user's calendar: confirmed, like sending.
        let when = start.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened).locale(Language.locale))
        try await requestConfirmation(
            result: .result(dialog: "\(t("intent_event_confirm", ["title": title, "when": when]))")
        )
        let format = DateFormatter()
        format.locale = Locale(identifier: "en_US_POSIX")
        format.dateFormat = "yyyy-MM-dd'T'HH:mm"
        let end = start.addingTimeInterval(TimeInterval(minutes * 60))
        let draft = EventDraft(summary: title, location: location ?? "", description: "", allDay: false,
                               start: format.string(from: start), end: format.string(from: end), repeat: .never)
        do {
            _ = try await engine.saveEventFor(account: from, calendarUrl: calendar.url, draft: draft)
        } catch {
            throw BridgeError(message: describe(error))
        }
        return .result(dialog: "\(t("intent_event_added"))")
    }
}

// MARK: acting on mail

extension Bridge {
    /// Acting on the user's behalf (changing mail) is Ferrowl Pro.
    static func requirePro(_ capability: Capability) async throws {
        let app = try app()
        guard capabilityNeedsPro(capability: capability) else { return }
        if !app.pro.pro { await app.pro.refresh() }
        guard app.pro.pro else { throw BridgeError(message: t("intent_needs_pro_write")) }
    }

    /// Runs `action` for each account and folder of `items`, on an engine
    /// of its own per account (the app's may be busy, or on another one).
    static func each(_ items: [MailEntity],
                     _ action: @escaping (Engine, String, [UInt32]) async throws -> Void) async throws {
        let app = try app()
        let accounts = try await accounts(nil)
        for (email, mine) in Dictionary(grouping: items, by: { $0.account.lowercased() }) {
            guard let account = accounts.first(where: { $0.email.lowercased() == email }) else {
                throw BridgeError(message: t("intent_no_account"))
            }
            let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
            do {
                _ = try await engine.connect(account: account)
                for (folder, inFolder) in Dictionary(grouping: mine, by: \.folder) {
                    try await action(engine, folder, inFolder.map(\.uid))
                }
            } catch {
                throw BridgeError(message: describe(error))
            }
        }
        app.mail.touch()
    }
}

struct MarkMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_mark_title"
    static var description = IntentDescription("intent_mark_desc")

    @Parameter(title: "intent_messages")
    var messages: [MailEntity]

    @Parameter(title: "intent_read", default: true)
    var read: Bool

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        try await Bridge.requirePro(.write)
        guard !messages.isEmpty else { return .result(dialog: "\(t("intent_done"))") }
        if capabilityNeedsConfirmation(capability: .write) {
            let key = read ? "intent_mark_read_confirm" : "intent_mark_unread_confirm"
            try await requestConfirmation(result: .result(dialog: "\(t(key, ["n": "\(messages.count)"]))"))
        }
        let read = read
        try await Bridge.each(messages) { engine, folder, uids in
            try await engine.setSeenMany(folderId: folder, uids: uids, seen: read)
        }
        return .result(dialog: "\(t("intent_done"))")
    }
}

struct ArchiveMailIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_archive_title"
    static var description = IntentDescription("intent_archive_desc")

    @Parameter(title: "intent_messages")
    var messages: [MailEntity]

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        try await Bridge.requirePro(.write)
        guard !messages.isEmpty else { return .result(dialog: "\(t("intent_done"))") }
        if capabilityNeedsConfirmation(capability: .write) {
            try await requestConfirmation(
                result: .result(dialog: "\(t("intent_archive_confirm", ["n": "\(messages.count)"]))")
            )
        }
        try await Bridge.each(messages) { engine, folder, uids in
            _ = try await engine.archiveMany(folderId: folder, uids: uids)
        }
        return .result(dialog: "\(t("intent_done"))")
    }
}

// MARK: mail details

/// A message's content and what it is about, for the following actions of
/// a shortcut (the text, the tracking number, the code, the due date…).
struct MailDetails: TransientAppEntity {
    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "intent_details")

    @Property(title: "intent_from") var sender: String
    @Property(title: "intent_subject") var subject: String
    @Property(title: "intent_date") var date: Date
    @Property(title: "intent_text") var text: String
    @Property(title: "intent_attachments") var attachments: [String]
    @Property(title: "intent_tracking") var tracking: String
    @Property(title: "intent_carrier") var carrier: String
    @Property(title: "intent_tracking_link") var trackingLink: URL?
    @Property(title: "insight_code") var code: String
    @Property(title: "intent_amount") var amount: String
    @Property(title: "intent_due") var due: Date?

    init() {
        sender = ""
        subject = ""
        date = Date()
        text = ""
        attachments = []
        tracking = ""
        carrier = ""
        trackingLink = nil
        code = ""
        amount = ""
        due = nil
    }

    var displayRepresentation: DisplayRepresentation {
        DisplayRepresentation(title: "\(subject.isEmpty ? t("no_subject") : subject)", subtitle: "\(sender)")
    }
}

struct MailDetailsIntent: AppIntent {
    static var title: LocalizedStringResource = "intent_details_title"
    static var description = IntentDescription("intent_details_desc")

    @Parameter(title: "intent_message")
    var message: MailEntity

    @MainActor
    func perform() async throws -> some IntentResult & ReturnsValue<MailDetails> {
        let app = try Bridge.app()
        guard let account = try await Bridge.accounts(nil).first(where: {
            $0.email.caseInsensitiveCompare(message.account) == .orderedSame
        }) else { throw BridgeError(message: t("intent_no_account")) }
        // Read without marking it read: a shortcut looking is not the user reading.
        let full: Message
        do {
            full = try await app.engine.peekMessage(account: account, folderId: message.folder, uid: message.uid)
        } catch {
            throw BridgeError(message: describe(error))
        }
        var details = MailDetails()
        details.sender = full.from.isEmpty ? full.fromAddress : full.from
        details.subject = full.subject
        details.date = Date(timeIntervalSince1970: TimeInterval(full.timestamp))
        details.text = full.text
        details.attachments = full.attachments.map(\.name)
        for insight in mailInsights(message: full) {
            switch insight {
            case .shipment(let carrier, let number, let url):
                details.tracking = number
                details.carrier = carrier
                details.trackingLink = URL(string: url)
            case .code(let code):
                details.code = code
            case .payment(_, let amount, let due):
                details.amount = amount
                let format = DateFormatter()
                format.locale = Locale(identifier: "en_US_POSIX")
                format.dateFormat = "yyyy-MM-dd"
                details.due = format.date(from: due)
            }
        }
        return .result(value: details)
    }
}

// MARK: Focus

/// Settings › Focus › (a Focus) › Filters › Ferrowl: the accounts that
/// Focus shows. The others leave the sidebar, and their notifications are
/// held back by iOS (they carry their address as `filterCriteria`).
struct FerrowlFocusFilter: SetFocusFilterIntent {
    static var title: LocalizedStringResource = "focus_title"
    static var description: IntentDescription? = IntentDescription("focus_desc")

    @Parameter(title: "focus_accounts")
    var accounts: [AccountEntity]?

    var displayRepresentation: DisplayRepresentation {
        let names = (accounts ?? []).map(\.id).joined(separator: ", ")
        return DisplayRepresentation(title: "\(names.isEmpty ? t("focus_all") : names)")
    }

    var appContext: FocusFilterAppContext {
        let allowed = (accounts ?? []).map { $0.id.lowercased() }
        return FocusFilterAppContext(
            notificationFilterPredicate: allowed.isEmpty ? nil : NSPredicate(format: "SELF IN %@", allowed)
        )
    }

    @MainActor
    func perform() async throws -> some IntentResult {
        let allowed = (accounts ?? []).map(\.id)
        FocusAccounts.allowed = allowed
        AppModel.shared?.mail.focus = FocusAccounts.allowed
        return .result()
    }
}

// MARK: Siri

struct FerrowlShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: UnreadMailIntent(),
            phrases: ["Unread mail in \(.applicationName)"],
            shortTitle: "intent_unread_title",
            systemImageName: "envelope.badge"
        )
        AppShortcut(
            intent: SearchMailIntent(),
            phrases: ["Search mail in \(.applicationName)"],
            shortTitle: "intent_search_title",
            systemImageName: "magnifyingglass"
        )
        AppShortcut(
            intent: ComposeMailIntent(),
            phrases: ["Write a mail with \(.applicationName)"],
            shortTitle: "intent_compose_title",
            systemImageName: "square.and.pencil"
        )
        AppShortcut(
            intent: AgendaIntent(),
            phrases: ["My agenda in \(.applicationName)"],
            shortTitle: "intent_agenda_title",
            systemImageName: "calendar"
        )
        AppShortcut(
            intent: FindContactIntent(),
            phrases: ["Find a contact in \(.applicationName)"],
            shortTitle: "intent_contact_title",
            systemImageName: "person.crop.circle"
        )
        AppShortcut(
            intent: NewEventIntent(),
            phrases: ["New event in \(.applicationName)"],
            shortTitle: "intent_new_event_title",
            systemImageName: "calendar.badge.plus"
        )
        AppShortcut(
            intent: SendMailIntent(),
            phrases: ["Send a mail with \(.applicationName)"],
            shortTitle: "intent_send_title",
            systemImageName: "paperplane"
        )
    }
}
