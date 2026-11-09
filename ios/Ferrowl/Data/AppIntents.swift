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
    static func accounts(_ wanted: AccountEntity?) throws -> [Account] {
        let app = try app()
        let usable = app.mail.accounts.filter { app.mayUse($0.email) }
        guard !usable.isEmpty else { throw BridgeError(message: t("intent_no_account")) }
        guard let wanted else { return usable }
        let chosen = usable.filter { $0.email.caseInsensitiveCompare(wanted.id) == .orderedSame }
        guard !chosen.isEmpty else { throw BridgeError(message: t("intent_no_account")) }
        return chosen
    }

    /// The account to send from: the one asked for, or the one in use.
    static func sender(_ wanted: AccountEntity?) throws -> Account {
        let app = try app()
        let usable = try accounts(wanted)
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
        for a in try Bridge.accounts(account) {
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
        let accounts = try Bridge.accounts(account)
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
        let from = try Bridge.sender(account)
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

    @MainActor
    func perform() async throws -> some IntentResult {
        let app = try Bridge.app()
        app.pendingCompose = ComposeRequest(to: to ?? "", subject: subject ?? "", body: body ?? "")
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
            intent: SendMailIntent(),
            phrases: ["Send a mail with \(.applicationName)"],
            shortTitle: "intent_send_title",
            systemImageName: "paperplane"
        )
    }
}
