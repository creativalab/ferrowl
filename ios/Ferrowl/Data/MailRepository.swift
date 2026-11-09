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

    /// Another account is being connected (the screen shows it).
    private(set) var switching: String?

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
    }

    // MARK: accounts

    /// Checks settings without saving them. Returns a confirmation text.
    func test(_ account: Account) async throws -> String {
        try await engine.test(account: account)
    }

    /// Connects and, if it works, saves the account (it becomes active).
    func setUp(_ account: Account) async throws {
        connecting?.cancel()
        connecting = nil
        folders = try await engine.connect(account: account)
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
        folders = try await engine.connect(account: account)
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

    /// Changes the saved settings of an account (same address).
    func update(_ account: Account) {
        guard let i = accounts.firstIndex(where: { $0.email == account.email }) else { return }
        accounts[i] = account
        persist()
    }

    /// Connects the active account once per process (concurrent callers
    /// wait for the same connection).
    @discardableResult
    func ensureConnected() async throws -> Bool {
        if !folders.isEmpty { return true }
        if let task = connecting {
            folders = try await task.value
            return true
        }
        guard let account = activeAccount else { return false }
        let engine = engine
        let task = Task { try await engine.connect(account: account) }
        connecting = task
        defer { connecting = nil }
        folders = try await task.value
        afterConnect()
        return true
    }

    /// Before an action on the server: the list can be on screen before the
    /// connection is made (it comes from the phone's copy), so wait for it.
    private func ready() async throws {
        if folders.isEmpty { try await ensureConnected() }
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

    /// Messages already on the phone.
    func cachedHeaders(_ folderId: String) async -> [Header] {
        let engine = engine
        return await Task.detached { engine.cachedHeaders(folderId: folderId) }.value
    }

    /// Newest messages from the server.
    func headers(_ folderId: String) async throws -> [Header] {
        try await ready()
        return try await engine.headers(folderId: folderId).headers
    }

    func search(_ folderId: String, _ query: String) async -> [Header] {
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
        let engine = engine
        return await Task.detached { engine.suggestAddresses(query: query) }.value
    }

    // MARK: actions

    func setSeen(_ folderId: String, _ uids: [UInt32], _ seen: Bool) async throws {
        try await ready()
        try await engine.setSeenMany(folderId: folderId, uids: uids, seen: seen)
        changes += 1
    }

    func setFlagged(_ folderId: String, _ uids: [UInt32], _ flagged: Bool) async throws {
        try await ready()
        try await engine.setFlaggedMany(folderId: folderId, uids: uids, flagged: flagged)
    }

    /// Archives messages; `quiet` when the screen already confirmed it (swipe).
    func archive(_ folderId: String, _ uids: [UInt32], quiet: Bool = false) async throws {
        try await ready()
        _ = try await engine.archiveMany(folderId: folderId, uids: uids)
        changed(quiet ? "" : uids.count == 1 ? t("archived") : t("bulk_archived", ["n": "\(uids.count)"]))
    }

    /// Moves to the Trash, or deletes for good when already there; `quiet`
    /// when the screen already confirmed it (swipe).
    func delete(_ folderId: String, _ uids: [UInt32], quiet: Bool = false) async throws {
        try await ready()
        let n = "\(uids.count)"
        if deletesForever(folderId) {
            try await engine.deleteForeverMany(folderId: folderId, uids: uids)
            changed(quiet ? "" : uids.count == 1 ? t("deleted_forever") : t("bulk_deleted_forever", ["n": n]))
        } else if let trash = folder(of: .trash) {
            try await engine.moveMany(folderId: folderId, uids: uids, targetId: trash.id)
            changed(quiet ? "" : uids.count == 1 ? t("moved_to_trash") : t("bulk_trashed", ["n": n]))
        }
    }

    func move(_ folderId: String, _ uids: [UInt32], to target: String) async throws {
        try await ready()
        try await engine.moveMany(folderId: folderId, uids: uids, targetId: target)
        changed(uids.count == 1 ? t("moved_ok") : t("bulk_moved", ["n": "\(uids.count)"]))
    }

    func setLabel(_ folderId: String, _ uids: [UInt32], _ label: String, _ on: Bool) async throws {
        try await ready()
        try await engine.setLabelMany(folderId: folderId, uids: uids, label: label, on: on)
    }

    /// Moves messages to Spam, or back to the inbox (`toSpam` false).
    func spam(_ folderId: String, _ uids: [UInt32], toSpam: Bool) async throws {
        try await ready()
        if toSpam {
            _ = try await engine.reportSpam(folderId: folderId, uids: uids)
        } else {
            _ = try await engine.notSpam(folderId: folderId, uids: uids)
        }
        changed(t(toSpam ? "spam_moved" : "inbox_moved", ["n": "\(uids.count)"]))
    }

    /// Sends; Markdown formatting typed by the user also goes out as HTML,
    /// as on desktop. A warning is shown when the copy in Sent failed.
    func send(_ draft: Outgoing) async throws {
        try await ready()
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
    func applySenderRules(_ folderId: String, _ headers: [Header]) async throws -> [UInt32] {
        let moved = try await engine.applySenderRules(folderId: folderId, headers: headers)
        if !moved.isEmpty { notices.info(t("rules_moved", ["n": "\(moved.count)"])) }
        return moved
    }

    // MARK: contacts and calendar

    func contacts(_ query: String) async -> [Contact] {
        let engine = engine
        return await Task.detached { engine.contacts(query: query) }.value
    }

    func syncContacts() async throws -> UInt32 {
        try await ready()
        return try await engine.syncContacts()
    }

    func agenda(from: Int64, to: Int64) async -> [CalendarEvent] {
        let engine = engine
        return await Task.detached { engine.agenda(from: from, to: to) }.value
    }

    func calendars() async -> [CalendarInfo] {
        let engine = engine
        return await Task.detached { engine.calendars() }.value
    }

    func eventDraft(_ url: String) async -> EventDraft? {
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
