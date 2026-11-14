import Foundation
import Observation

/// The single entry point to mail for the screens: it wraps the Rust
/// [Engine] and the saved accounts. Screens and their models talk to this
/// class, never to the engine directly (as `MailRepository` on Android).
///
/// Engine calls are `async`: they run off the main thread and throw
/// `FerrowlError` with a message ready to show ([describe]). The engine's
/// quick local reads (cache, search) are moved off the main thread here.
@MainActor
@Observable
final class MailRepository {
    let engine: Engine
    let notices: Notices
    private let store: AccountStore

    /// Every account on the phone, in the order they were added.
    private(set) var accounts: [Account] = []
    /// The accounts the current Focus shows (nil: all of them).
    var focus: [String]? = FocusAccounts.allowed

    /// The accounts to show: those of the current Focus, or all.
    var visibleAccounts: [Account] {
        guard let focus else { return accounts }
        let shown = accounts.filter { focus.contains($0.email.lowercased()) }
        return shown.isEmpty ? accounts : shown
    }

    /// The active account's address (nil with no account).
    private(set) var activeEmail: String?

    /// Folders of the connected account (empty until [ensureConnected]).
    private(set) var folders: [Folder] = []

    /// Goes up after messages were moved, deleted or sent: lists reload.
    private(set) var changes = 0
    /// Goes up when the server says the account in use has new mail (or
    /// mail read elsewhere) while the app is open: its inbox is fetched again.
    private(set) var arrivals = 0

    /// Goes up after another account became the active one: screens start over.
    private(set) var switches = 0

    private var connecting: Task<[Folder], Error>?
    /// The account the engine is connected to (its folders are `folders`).
    private var connected: String?

    /// Another account is being connected (the screen shows it).
    private(set) var switching: String?

    /// A message being written is open: switches the app makes by itself
    /// (a notification of another account, a Focus, the end of Ferrowl Pro)
    /// wait for it to close, as switching starts the screens over.
    var composing = false

    /// Waits until no message being written is open.
    func afterCompose() async {
        while composing, !Task.isCancelled { try? await Task.sleep(for: .milliseconds(400)) }
    }

    init(engine: Engine, notices: Notices, directory: URL) {
        self.engine = engine
        self.notices = notices
        store = AccountStore(directory: directory)
        let saved = store.load()
        accounts = saved.accounts
        activeEmail = saved.active
    }

    var activeAccount: Account? {
        accounts.first { $0.email.caseInsensitiveCompare(activeEmail ?? "") == .orderedSame }
    }

    private func persist() {
        store.save(AccountStore.Snapshot(accounts: accounts, active: activeEmail))
        WidgetData.keep(accounts.map(\.email))
    }

    // MARK: accounts

    /// Checks settings without saving them. Returns a confirmation text.
    func test(_ account: Account) async throws -> String {
        try await engine.test(account: account)
    }

    /// Connects and, if it works, saves the account (it becomes active).
    func setUp(_ account: Account) async throws {
        await flushDeferred()
        // After any connection in progress (see `switchTo`).
        while let task = connecting { _ = try? await task.value }
        let task = Task { try await engine.connect(account: account) }
        connecting = task
        defer { connecting = nil }
        folders = try await task.value
        connected = account.email
        accounts.removeAll { $0.email.caseInsensitiveCompare(account.email) == .orderedSame }
        accounts.append(account)
        activeEmail = account.email
        persist()
        switches += 1
        afterConnect()
    }

    /// Makes another account the active one and connects it.
    func switchTo(_ email: String) async throws {
        guard let account = accounts.first(where: { $0.email.caseInsensitiveCompare(email) == .orderedSame }) else { return }
        switching = account.email
        defer { switching = nil }
        await flushDeferred()
        // One connection at a time: one still in progress (the previous
        // account, at start-up) would otherwise finish last and leave the
        // engine on it, so a message of this account would be fetched (and
        // marked read) by its number from the other one's inbox.
        while let task = connecting { _ = try? await task.value }
        let task = Task { try await engine.connect(account: account) }
        connecting = task
        defer { connecting = nil }
        folders = try await task.value
        connected = account.email
        activeEmail = account.email
        persist()
        switches += 1
        afterConnect()
    }

    /// Without Ferrowl Pro only the first account may be used: it becomes
    /// the active one again (saved before connecting, so a failed
    /// connection is retried with the right account).
    func backToFirst() async {
        guard let first = accounts.first, first.email != activeEmail else { return }
        await flushDeferred()
        guard first.email != activeEmail else { return }
        activeEmail = first.email
        folders = []
        persist()
        switches += 1
        _ = try? await ensureConnected()
    }

    /// Removes an account from the phone (its sign-in and cached mail too);
    /// another one, if any, becomes active. Returns false when none is left.
    func removeAccount(_ email: String) async -> Bool {
        guard let removed = accounts.first(where: { $0.email.caseInsensitiveCompare(email) == .orderedSame }) else {
            return !accounts.isEmpty
        }
        await flushDeferred()
        if let provider = removed.oauth { oauthForget(provider: provider, email: removed.email) }
        // The same mailbox added twice shares its cache: kept then.
        let sharesCache = accounts.contains {
            $0.email != removed.email && $0.username == removed.username && $0.incomingHost == removed.incomingHost
        }
        if !sharesCache {
            let engine = engine
            await Task.detached { engine.forgetAccount(account: removed) }.value
        }
        store.removeSecrets(of: removed.email)
        Badge.forget(removed.email)
        SpotlightIndex.remove(account: removed.email)
        Badge.show()
        accounts.removeAll { $0.email == removed.email }
        let wasActive = removed.email == activeEmail
        if wasActive { activeEmail = accounts.first?.email }
        persist()
        if accounts.isEmpty {
            folders = []
            return false
        }
        if wasActive {
            folders = []
            switches += 1
            _ = try? await ensureConnected()
        }
        return true
    }

    /// Changes the saved settings of an account (same address). The account
    /// in use connects again, so its address book and calendars follow the
    /// new servers (the engine keeps the settings it was connected with).
    func update(_ account: Account) async {
        guard let i = accounts.firstIndex(where: { $0.email == account.email }) else { return }
        let old = accounts[i]
        accounts[i] = account
        persist()
        let servers = old.displayName != account.displayName || old.carddavUrl != account.carddavUrl || old.caldavUrl != account.caldavUrl
            || old.davUsername != account.davUsername || old.davPassword != account.davPassword
        guard servers, account.email == activeEmail, connected == account.email else { return }
        while let task = connecting { _ = try? await task.value }
        guard account.email == activeEmail, connected == account.email else { return }
        let engine = engine
        let task = Task { try await engine.connect(account: account) }
        connecting = task
        defer { connecting = nil }
        guard let list = try? await task.value else {
            // Connected again at the next action.
            folders = []
            return
        }
        folders = list
        afterConnect()
    }

    /// Connects the active account once per process (concurrent callers
    /// wait for the same connection).
    @discardableResult
    func ensureConnected() async throws -> Bool {
        while true {
            if !folders.isEmpty, connected == activeEmail { return true }
            // Another connection (a switch of account) first, then check again.
            if let task = connecting {
                _ = try? await task.value
                continue
            }
            guard let account = activeAccount else { return false }
            let engine = engine
            let task = Task { try await engine.connect(account: account) }
            connecting = task
            defer { connecting = nil }
            let list = try await task.value
            // Still the account in use (not switched meanwhile).
            if account.email == activeEmail {
                folders = list
                connected = account.email
                afterConnect()
                return true
            }
        }
    }

    /// Before an action on the server: the list can be on screen before the
    /// connection is made (it comes from the phone's copy), so wait for it.
    private func ready() async throws {
        if folders.isEmpty || connected != activeEmail { try await ensureConnected() }
    }

    /// `ready`, for an action asked on `account`: refused when the app has
    /// moved to another account meanwhile, or the same message numbers
    /// would be changed (or deleted) in the other mailbox.
    @discardableResult
    private func ready(for account: String?) async throws -> String {
        guard let account, isCurrent(account) else { throw AccountChanged() }
        try await ready()
        guard isCurrent(account), connected?.caseInsensitiveCompare(account) == .orderedSame else { throw AccountChanged() }
        return account
    }

    /// An engine action given the account it is meant for (`…ForAccount`):
    /// the engine itself refuses when it is on another one (a switch while
    /// the call waited), which counts as `AccountChanged` here too.
    private func guarded<T>(_ call: () async throws -> T) async throws -> T {
        do {
            return try await call()
        } catch let error as FerrowlError {
            if case .Failed(let message) = error, message == t("err_other_account") { throw AccountChanged() }
            throw error
        }
    }

    /// Whether `account` is still the one in use.
    func isCurrent(_ account: String?) -> Bool {
        guard let account, let activeEmail else { return false }
        return account.caseInsensitiveCompare(activeEmail) == .orderedSame
    }

    // MARK: actions waiting for Undo

    private struct Deferred {
        let account: String
        let action: () async -> Void
        let timer: Task<Void, Never>
    }

    private var deferred: [UUID: Deferred] = [:]

    /// Runs `action` (a server change on `account`) after `delay`, unless
    /// cancelled first (Undo). It belongs to the account, not to a screen:
    /// before the engine moves to another account it runs at once.
    func later(on account: String, after delay: Duration, _ action: @escaping () async -> Void) -> UUID {
        let id = UUID()
        let timer = Task { [weak self] in
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            await self?.runDeferred(id)
        }
        deferred[id] = Deferred(account: account, action: action, timer: timer)
        return id
    }

    /// Undo: false when the action already ran.
    func cancelLater(_ id: UUID) -> Bool {
        guard let job = deferred.removeValue(forKey: id) else { return false }
        job.timer.cancel()
        return true
    }

    private func runDeferred(_ id: UUID) async {
        guard let job = deferred.removeValue(forKey: id) else { return }
        await job.action()
    }

    /// Before connecting another account: what waits for Undo is done now,
    /// while the engine is still on its account. Also when the app leaves
    /// the screen (it may be stopped before the notice is gone).
    func flushDeferred() async {
        guard !deferred.isEmpty else { return }
        while let (id, job) = deferred.first {
            deferred.removeValue(forKey: id)
            job.timer.cancel()
            await job.action()
        }
    }

    /// Once per connection: the address book learns who we write to from
    /// Sent, and contacts and calendars come from CardDAV/CalDAV. Most
    /// accounts have no such server, so failures are not shown here.
    private func afterConnect() {
        let engine = engine
        let sent = folder(of: .sent)?.id
        Task.detached {
            if let sent { _ = try? await engine.headers(folderId: sent) }
            _ = try? await engine.syncContacts()
            _ = try? await engine.syncCalendar()
        }
    }

    // MARK: folders

    func folder(_ id: String) -> Folder? { folders.first { $0.id == id } }

    func folder(of kind: FolderKind) -> Folder? { folders.first { $0.kind == kind } }

    var inboxId: String { folder(of: .inbox)?.id ?? "INBOX" }

    /// Whether deleting from this folder is final (it is the Trash).
    func deletesForever(_ folderId: String) -> Bool {
        folder(folderId)?.kind == .trash || folder(of: .trash) == nil
    }

    func isSpam(_ folderId: String) -> Bool { folder(folderId)?.kind == .spam }

    /// Whether the account is Gmail (messages have labels, the inbox has tabs).
    var isGmail: Bool {
        folders.isEmpty ? (activeAccount?.incomingHost.hasSuffix("gmail.com") ?? false) : engine.isGmail()
    }

    /// Gmail labels that can be put on messages of a folder: the user's own.
    func labelChoices(_ folderId: String) -> [Folder] {
        folders.filter {
            $0.kind == .other && $0.selectable && $0.id != folderId
                && !$0.id.hasPrefix("[Gmail]") && !$0.id.hasPrefix("[Google Mail]")
        }
    }

    // MARK: reading

    /// The active account's inbox as saved on the phone, before connecting.
    func cachedInbox() async -> [Header] {
        guard let account = activeAccount else { return [] }
        let engine = engine
        return await Task.detached { engine.cachedInbox(account: account) }.value
    }

    /// The engine reads the phone's copy of the account it is connected
    /// to: with none yet (the app just started, e.g. from a notification)
    /// those reads come back empty, so the connection is made first. Offline
    /// they still give what there is.
    private func readable() async {
        if folders.isEmpty || connected != activeEmail { _ = try? await ensureConnected() }
    }

    /// Messages already on the phone.
    func cachedHeaders(_ folderId: String) async -> [Header] {
        let engine = engine
        return await Task.detached { engine.cachedHeaders(folderId: folderId) }.value
    }

    /// Newest messages from the server; with `account`, refused when the
    /// app moved to another one meanwhile.
    func headers(_ folderId: String, on account: String? = nil) async throws -> [Header] {
        guard let account else {
            try await ready()
            return try await engine.headers(folderId: folderId).headers
        }
        try await ready(for: account)
        return try await guarded { try await engine.headersForAccount(account: account, folderId: folderId).headers }
    }

    func search(_ folderId: String, _ query: String) async -> [Header] {
        await readable()
        let engine = engine
        return await Task.detached { engine.search(folderId: folderId, query: query) }.value
    }

    /// Unread count of every folder.
    func unread() async throws -> [String: UInt32] {
        try await ready()
        return try await engine.unread(folderIds: folders.filter(\.selectable).map(\.id))
    }

    func message(_ folderId: String, _ uid: UInt32) async throws -> Message {
        try await ready()
        return try await engine.message(folderId: folderId, uid: uid)
    }

    func suggest(_ query: String) async -> [AddressSuggestion] {
        await readable()
        let engine = engine
        return await Task.detached { engine.suggestAddresses(query: query) }.value
    }

    // MARK: actions

    func setSeen(_ folderId: String, _ uids: [UInt32], _ seen: Bool, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        try await guarded { try await engine.setSeenManyForAccount(account: account, folderId: folderId, uids: uids, seen: seen) }
        changes += 1
    }

    func setFlagged(_ folderId: String, _ uids: [UInt32], _ flagged: Bool, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        try await guarded { try await engine.setFlaggedManyForAccount(account: account, folderId: folderId, uids: uids, flagged: flagged) }
    }

    /// Archives messages; `quiet` when the screen already confirmed it (swipe).
    func archive(_ folderId: String, _ uids: [UInt32], quiet: Bool = false, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        _ = try await guarded { try await engine.archiveManyForAccount(account: account, folderId: folderId, uids: uids) }
        changed(quiet ? "" : uids.count == 1 ? t("archived") : t("bulk_archived", ["n": "\(uids.count)"]))
    }

    /// Moves to the Trash, or deletes for good when already there; `quiet`
    /// when the screen already confirmed it (swipe).
    func delete(_ folderId: String, _ uids: [UInt32], quiet: Bool = false, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        let n = "\(uids.count)"
        if deletesForever(folderId) {
            try await guarded { try await engine.deleteForeverManyForAccount(account: account, folderId: folderId, uids: uids) }
            changed(quiet ? "" : uids.count == 1 ? t("deleted_forever") : t("bulk_deleted_forever", ["n": n]))
        } else if let trash = folder(of: .trash) {
            try await guarded { try await engine.moveManyForAccount(account: account, folderId: folderId, uids: uids, targetId: trash.id) }
            changed(quiet ? "" : uids.count == 1 ? t("moved_to_trash") : t("bulk_trashed", ["n": n]))
        }
    }

    func move(_ folderId: String, _ uids: [UInt32], to target: String, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        try await guarded { try await engine.moveManyForAccount(account: account, folderId: folderId, uids: uids, targetId: target) }
        changed(uids.count == 1 ? t("moved_ok") : t("bulk_moved", ["n": "\(uids.count)"]))
    }

    func setLabel(_ folderId: String, _ uids: [UInt32], _ label: String, _ on: Bool, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        try await guarded { try await engine.setLabelManyForAccount(account: account, folderId: folderId, uids: uids, label: label, on: on) }
    }

    /// Moves messages to Spam, or back to the inbox (`toSpam` false).
    func spam(_ folderId: String, _ uids: [UInt32], toSpam: Bool, on account: String? = nil) async throws {
        let account = try await ready(for: account ?? activeEmail)
        if toSpam {
            _ = try await guarded { try await engine.reportSpamForAccount(account: account, folderId: folderId, uids: uids) }
        } else {
            _ = try await guarded { try await engine.notSpamForAccount(account: account, folderId: folderId, uids: uids) }
        }
        changed(t(toSpam ? "spam_moved" : "inbox_moved", ["n": "\(uids.count)"]))
    }

    /// Sends; Markdown formatting typed by the user also goes out as HTML,
    /// as on desktop. A warning is shown when the copy in Sent failed.
    func send(_ draft: Outgoing, on account: String? = nil) async throws {
        try await ready(for: account ?? activeEmail)
        var draft = draft
        draft.html = markdownToHtml(markdown: draft.body)
        let warning = try await engine.send(message: draft)
        changed(warning.map { t("sent_no_copy", ["err": $0]) } ?? t("sent_ok"))
    }

    // MARK: sender lists

    /// Blocks (false) or allows (true) a sender: an address or "@domain".
    func setSenderRule(_ address: String, allow: Bool) throws {
        try engine.setSenderRule(address: address, allow: allow)
    }

    func removeSenderRule(_ address: String) { engine.removeSenderRule(address: address) }

    func senderRules(allow: Bool) -> [String] { engine.senderRules(allow: allow) }

    /// Applies the sender lists to a freshly loaded folder; returns the
    /// messages moved (to drop from the list).
    func applySenderRules(_ folderId: String, _ headers: [Header], on account: String) async throws -> [UInt32] {
        try await ready(for: account)
        let moved = try await guarded { try await engine.applySenderRulesForAccount(account: account, folderId: folderId, headers: headers) }
        if !moved.isEmpty { notices.info(t("rules_moved", ["n": "\(moved.count)"])) }
        return moved
    }

    // MARK: rules

    /// The inbox's new messages through the user's rules (each once).
    func applyRules(_ folderId: String, _ headers: [Header], on account: String) async -> RulesOutcome? {
        guard folderId == inboxId, !headers.isEmpty else { return nil }
        // Another account's rules must not run on these numbers (nor its
        // "done up to" mark move to this account's newest message).
        guard (try? await ready(for: account)) != nil else { return nil }
        guard let outcome = try? await engine.applyRulesForAccount(account: account, folderId: folderId, headers: headers) else { return nil }
        let touched = Set(outcome.removed + outcome.read + outcome.starred)
        if !touched.isEmpty { notices.info(t("rules_applied", ["n": "\(touched.count)"])) }
        return outcome
    }

    /// The active account's rules.
    func rules() -> [Rule] {
        guard let account = activeAccount else { return [] }
        return engine.rules(account: account)
    }

    func saveRule(_ rule: Rule) throws {
        guard let account = activeAccount else { return }
        try engine.saveRule(account: account, rule: rule)
    }

    func deleteRule(_ id: String) throws {
        guard let account = activeAccount else { return }
        try engine.deleteRule(account: account, id: id)
    }

    /// The inbox messages on the phone a condition picks.
    func rulePreview(_ condition: String) async -> [Header] {
        guard let account = activeAccount else { return [] }
        let engine = engine
        return await Task.detached { engine.rulePreview(account: account, condition: condition, limit: 5) }.value
    }

    // MARK: contacts and calendar

    func contacts(_ query: String) async -> [Contact] {
        await readable()
        let engine = engine
        return await Task.detached { engine.contacts(query: query) }.value
    }

    /// Saves a contact written in the app (on the contacts server when the
    /// account has one, otherwise on the phone).
    func saveContact(_ contact: Contact) async throws {
        try await ready()
        _ = try await engine.saveContact(contact: contact)
    }

    func deleteContact(_ url: String) async throws {
        try await ready()
        try await engine.deleteContact(url: url)
    }

    func syncContacts() async throws -> UInt32 {
        try await ready()
        return try await engine.syncContacts()
    }

    func agenda(from: Int64, to: Int64) async -> [CalendarEvent] {
        await readable()
        let engine = engine
        return await Task.detached { engine.agenda(from: from, to: to) }.value
    }

    func calendars() async -> [CalendarInfo] {
        await readable()
        let engine = engine
        return await Task.detached { engine.calendars() }.value
    }

    func eventDraft(_ url: String) async -> EventDraft? {
        await readable()
        let engine = engine
        return await Task.detached { engine.eventDraft(url: url) }.value
    }

    func syncCalendar() async throws {
        try await ready()
        _ = try await engine.syncCalendar()
    }

    func saveEvent(calendarUrl: String, url: String?, draft: EventDraft) async throws -> String {
        try await ready()
        return try await engine.saveEvent(calendarUrl: calendarUrl, url: url, draft: draft)
    }

    func deleteEvent(_ url: String) async throws -> String {
        try await ready()
        return try await engine.deleteEvent(url: url)
    }

    // MARK: invitations

    func respondInvitation(_ calendar: String, _ reply: InviteReply) async throws -> String {
        try await ready()
        return try await engine.respondInvitation(calendar: calendar, reply: reply)
    }

    func invitationReply(_ uid: String) -> InviteReply? { engine.invitationReply(uid: uid) }

    func inCalendar(_ uid: String) -> Bool { engine.inCalendar(uid: uid) }

    func removeCancelled(_ uid: String) async throws -> String {
        try await ready()
        return try await engine.removeCancelled(uid: uid)
    }

    /// The inbox was refreshed elsewhere (new-mail check): lists reload.
    func touch() { changes += 1 }

    /// A push for `email` arrived while the app is open.
    func arrived(_ email: String) {
        if email.caseInsensitiveCompare(activeEmail ?? "") == .orderedSame { arrivals += 1 }
    }

    private func changed(_ notice: String) {
        changes += 1
        notices.info(notice)
    }
}
