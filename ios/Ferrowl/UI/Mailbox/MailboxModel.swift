import Foundation
import Observation

/// A row of the message list: a conversation, or one message.
struct Row: Identifiable, Hashable {
    let conversation: Conversation
    var id: UInt32 { conversation.latest.uid }

    // The same row whatever its state: it is the list's selection and the
    // open message, which must not change when the message becomes read
    // (SwiftUI then rebuilt and moved the reader, freezing for seconds).
    static func == (a: Row, b: Row) -> Bool { a.id == b.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
    var header: Header { conversation.latest }
    var uids: [UInt32] { conversation.uids }
    var unread: Bool { conversation.unread > 0 }
    var flagged: Bool { header.flagged }
}

/// The state of the mailbox (folders, list, search, selection), as
/// `MailboxViewModel` on Android.
@MainActor
@Observable
final class MailboxModel {
    let mail: MailRepository
    let prefs: Preferences
    let notices: Notices

    var folderId = ""
    var unread: [String: UInt32] = [:]
    private(set) var messages: [Header] = []
    private(set) var refreshing = false
    /// Search text; when not blank the list shows [results].
    var query = "" {
        didSet { if query != oldValue { search() } }
    }
    private(set) var results: [Header] = []
    /// Gmail inbox: which tab is open (0 Primary … 4 Forums).
    var category = 0
    /// Checking messages: the selection toolbar is shown.
    var selecting = false
    var selected: Set<UInt32> = []
    /// The open row (shown in the detail column).
    var open: Row?

    private var searchTask: Task<Void, Never>?
    private var started = false
    /// The folder being fetched from the server, and whether another fetch
    /// was asked meanwhile: fetches never overlap (each applies the rules,
    /// which must not run twice on the same new messages).
    private var fetching: String?
    private var fetchAgain = false
    /// Deleted with Undo but not yet on the server (it is asked when the
    /// notice is gone), per folder: lists loaded meanwhile from the local
    /// copy or the server must not show them again.
    private var pendingDeletes: [String: Set<UInt32>] = [:]

    /// `list` without the messages of `folder` waiting to be deleted.
    private func visible(_ list: [Header], in folder: String) -> [Header] {
        guard let gone = pendingDeletes[folder], !gone.isEmpty else { return list }
        return list.filter { !gone.contains($0.uid) }
    }

    /// The account this mailbox shows (the screen starts over for another).
    let account: String?
    /// Cleared when the screen goes: work still running (a refresh) stops
    /// there instead of touching the account that replaced it.
    var alive = true

    /// Still the mailbox on screen, for the account the engine is on.
    private var current: Bool { alive && mail.isCurrent(account) }

    init(mail: MailRepository, prefs: Preferences, notices: Notices) {
        self.mail = mail
        self.prefs = prefs
        self.notices = notices
        account = mail.activeEmail
    }

    var folder: Folder? { mail.folder(folderId) }

    /// Gmail inbox: the tabs are shown.
    var tabs: Bool { mail.isGmail && folderId == mail.inboxId && query.isEmpty }

    var shown: [Header] {
        if !query.trimmingCharacters(in: .whitespaces).isEmpty { return results }
        if tabs { return messages.filter { Int($0.category) == category } }
        return messages
    }

    /// The list's rows. Grouping and sorting go through the engine, so they
    /// are kept until the messages or the settings change: the list is drawn
    /// again for every tap (the selection), which must not redo them.
    var rows: [Row] {
        let list = shown
        let threaded = prefs.threaded
        let sort = prefs.sort
        if let cached = rowsCache, cached.threaded == threaded, cached.sort == sort, cached.list == list {
            return cached.rows
        }
        let conversations = threaded
            ? groupConversations(headers: list)
            : list.map { Conversation(latest: $0, uids: [$0.uid], unread: $0.seen ? 0 : 1) }
        let rows = sortConversations(rows: conversations, order: sort).map(Row.init)
        rowsCache = (list, threaded, sort, rows)
        return rows
    }

    @ObservationIgnored private var rowsCache: (list: [Header], threaded: Bool, sort: SortOrder, rows: [Row])?

    /// Gmail inbox: unread messages per tab.
    var unreadByCategory: [Int: Int] {
        guard tabs else { return [:] }
        return Dictionary(grouping: messages.filter { !$0.seen }, by: { Int($0.category) }).mapValues(\.count)
    }

    func header(_ uid: UInt32) -> Header? { messages.first { $0.uid == uid } ?? results.first { $0.uid == uid } }

    // MARK: loading

    /// The saved inbox right away (connecting can take a while), then the
    /// server.
    func start() async {
        guard !started else { return }
        started = true
        refreshing = true
        let early = await mail.cachedInbox()
        guard current else { return }
        if !early.isEmpty, folderId.isEmpty {
            folderId = "INBOX"
            messages = visible(early, in: "INBOX")
        }
        do {
            try await mail.ensureConnected()
            guard current else { return }
            if folderId.isEmpty || mail.folder(folderId) == nil { folderId = mail.inboxId }
            await refresh()
        } catch {
            refreshing = false
            notices.error(describe(error))
        }
    }

    func openFolder(_ id: String) {
        guard id != folderId else { return }
        folderId = id
        messages = []
        query = ""
        results = []
        selected = []
        selecting = false
        category = 0
        open = nil
        Task { await refresh() }
    }

    /// The local copy at once, then the newest messages from the server.
    /// The server has news for the account (a push while the app is open):
    /// the inbox, if open, is fetched again; elsewhere only the counts.
    func serverChanged() async {
        await update(onlyInbox: true)
    }

    /// The app came back to the screen: the open folder from the server
    /// (during a search, only the counts).
    func returned() async {
        guard started else { return }
        await update(onlyInbox: false)
    }

    /// Something changed without the user asking (a push, the app back on
    /// screen): the open folder again, quietly (no error notice when
    /// offline), or only the counts during a search or in another folder.
    private func update(onlyInbox: Bool) async {
        let searching = !query.trimmingCharacters(in: .whitespaces).isEmpty
        if searching || (onlyInbox && folderId != mail.inboxId) {
            await loadUnread()
        } else {
            await refresh(quiet: true)
        }
    }

    func refresh(quiet: Bool = false) async {
        let id = folderId
        guard !id.isEmpty else { return }
        if fetching == id {
            fetchAgain = true
            return
        }
        fetching = id
        defer {
            if fetching == id { fetching = nil }
        }
        refreshing = true
        defer { if folderId == id { refreshing = false } }
        await fetch(id, quiet: quiet)
        // Asked again while fetching (a push during the fetch): once more.
        while fetchAgain, folderId == id {
            fetchAgain = false
            await fetch(id, quiet: true)
        }
        fetchAgain = false
    }

    private func fetch(_ id: String, quiet: Bool) async {
        // After every wait: still this account (a switch meanwhile leaves
        // this screen behind, and the engine on the other account).
        guard current, let account else { return }
        let cached = await mail.cachedHeaders(id)
        guard current else { return }
        if !cached.isEmpty, folderId == id { messages = visible(cached, in: id) }
        do {
            var fresh = try await mail.headers(id, on: account)
            guard current else { return }
            // Sender lists: blocked mail leaves the inbox, allowed mail leaves Spam.
            if let moved = try? await mail.applySenderRules(id, fresh, on: account), !moved.isEmpty {
                fresh.removeAll { moved.contains($0.uid) }
            }
            guard current else { return }
            // The user's rules on the inbox's new messages.
            if let done = await mail.applyRules(id, fresh, on: account) {
                fresh.removeAll { done.removed.contains($0.uid) }
                for i in fresh.indices {
                    if done.read.contains(fresh[i].uid) { fresh[i].seen = true }
                    if done.starred.contains(fresh[i].uid) { fresh[i].flagged = true }
                }
            }
            guard current, folderId == id else { return }
            messages = visible(fresh, in: id)
            if id == mail.inboxId {
                // What the background check compares with.
                BackgroundRefresh.notifyNew(fresh, account: account, announce: false)
                // The inbox in the iPhone's search.
                if prefs.spotlight { SpotlightIndex.index(fresh, account: account) }
            }
            await loadUnread()
        } catch {
            if !quiet { notices.error(describe(error)) }
        }
    }

    /// Something was moved, deleted or sent: reload from the local copy.
    func reloadLocal() async {
        let id = folderId
        guard !id.isEmpty else { return }
        guard current else { return }
        let cached = await mail.cachedHeaders(id)
        guard current else { return }
        if folderId == id { messages = visible(cached, in: id) }
        await loadUnread()
    }

    func loadUnread() async {
        guard current else { return }
        if let counts = try? await mail.unread(), current {
            unread = counts
            // The number on the app icon (Gmail: from the inbox's messages,
            // so only when the inbox is the open folder).
            if let account = mail.activeAccount {
                let gmail = Badge.isGmail(account)
                if !gmail || folderId == mail.inboxId,
                   let n = Badge.unread(server: counts[mail.inboxId], headers: messages, gmail: gmail) {
                    // The inbox's messages for the widget (only when it is the open folder).
                    let inbox = folderId == mail.inboxId ? messages.filter { !gmail || $0.category == 0 } : nil
                    Badge.set(n, for: account.email, recent: inbox)
                }
                Badge.show()
            }
        }
    }

    private func search() {
        searchTask?.cancel()
        let text = query
        guard !text.trimmingCharacters(in: .whitespaces).isEmpty else {
            results = []
            return
        }
        let id = folderId
        searchTask = Task {
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
            let found = await mail.search(id, text)
            guard !Task.isCancelled else { return }
            results = visible(found, in: id)
        }
    }

    // MARK: selection

    func toggle(_ row: Row) {
        let uids = Set(row.uids)
        if uids.isSubset(of: selected) {
            selected.subtract(uids)
        } else {
            selected.formUnion(uids)
        }
    }

    func isSelected(_ row: Row) -> Bool { Set(row.uids).isSubset(of: selected) }

    enum Pick { case all, none, read, unread, starred }

    func select(_ pick: Pick) {
        selected = Set(shown.filter {
            switch pick {
            case .all: true
            case .none: false
            case .read: $0.seen
            case .unread: !$0.seen
            case .starred: $0.flagged
            }
        }.map(\.uid))
    }

    func endSelection() {
        selecting = false
        selected = []
    }

    // MARK: actions

    private func update(_ uids: [UInt32], _ change: (inout Header) -> Void) {
        let set = Set(uids)
        for i in messages.indices where set.contains(messages[i].uid) { change(&messages[i]) }
        for i in results.indices where set.contains(results[i].uid) { change(&results[i]) }
    }

    /// The selection, or the given row's messages.
    private func target(_ row: Row?) -> [UInt32] { row?.uids ?? Array(selected) }

    /// Runs a server action on this mailbox's account (refused by the
    /// repository if the app moved to another account by then).
    private func run(_ uids: [UInt32], _ action: @escaping (String, [UInt32], String?) async throws -> Void) {
        guard !uids.isEmpty else { return }
        let folder = folderId
        let account = account
        endSelection()
        Task {
            do {
                try await action(folder, uids, account)
            } catch is AccountChanged {
                // That account is not on screen any more: nothing to show.
            } catch {
                notices.error(describe(error))
                await refresh()
            }
        }
    }

    func markRead(_ seen: Bool, _ row: Row? = nil) {
        let uids = target(row)
        update(uids) { $0.seen = seen }
        run(uids) { [mail] f, u, a in try await mail.setSeen(f, u, seen, on: a) }
    }

    func star(_ on: Bool, _ row: Row? = nil) {
        let uids = target(row)
        update(uids) { $0.flagged = on }
        run(uids) { [weak self, mail] f, u, a in
            do {
                try await mail.setFlagged(f, u, on, on: a)
            } catch {
                self?.update(u) { $0.flagged = !on }
                throw error
            }
        }
    }

    func archive(_ row: Row? = nil) {
        run(target(row)) { [mail] f, u, a in try await mail.archive(f, u, on: a) }
    }

    /// Swipe, the row's menu or the trash button of an open message: the row
    /// leaves the list at once and "Moved to Trash · Undo" shows; the server
    /// is asked only when the notice is gone, so Undo really undoes it.
    func deleteWithUndo(_ row: Row) {
        let folder = folderId
        let uids = Set(row.uids)
        let removed = messages.filter { uids.contains($0.uid) }
        messages.removeAll { uids.contains($0.uid) }
        results.removeAll { uids.contains($0.uid) }
        if open?.id == row.id { open = nil }
        pendingDeletes[folder, default: []].formUnion(uids)
        guard let account else { return }
        // Kept by the repository, on this account: it runs when the notice
        // is gone, or at once before the app connects another account (and
        // never on the other account's messages with the same numbers).
        let job = mail.later(on: account, after: Notices.duration) { [weak self, mail, notices] in
            do {
                try await mail.delete(folder, Array(uids), quiet: true, on: account)
                self?.pendingDeletes[folder]?.subtract(uids)
            } catch is AccountChanged {
                self?.pendingDeletes[folder]?.subtract(uids)
            } catch {
                self?.pendingDeletes[folder]?.subtract(uids)
                notices.error(describe(error))
                await self?.refresh()
            }
        }
        notices.withAction(t("moved_to_trash"), t("undo")) { [weak self, mail] in
            // Already done (the app connected another account meanwhile).
            guard mail.cancelLater(job) else { return }
            self?.pendingDeletes[folder]?.subtract(uids)
            guard let self, self.current, self.folderId == folder else { return }
            self.messages = (self.messages + removed).sorted { $0.timestamp > $1.timestamp }
        }
    }

    var deletesForever: Bool { mail.deletesForever(folderId) }

    func delete(_ row: Row? = nil) {
        let uids = target(row)
        if let open, uids.contains(open.id) { self.open = nil }
        run(uids) { [mail] f, u, a in try await mail.delete(f, u, on: a) }
    }

    func move(to target: String, _ row: Row? = nil) {
        run(self.target(row)) { [mail] f, u, a in try await mail.move(f, u, to: target, on: a) }
    }

    var inSpam: Bool { mail.isSpam(folderId) }

    /// Report spam, or in Spam "not spam" (which also allows the senders).
    func spam(_ row: Row? = nil) {
        let uids = target(row)
        let toSpam = !inSpam
        if !toSpam {
            let set = Set(uids)
            let senders = Set(shown.filter { set.contains($0.uid) && !$0.fromAddress.isEmpty }.map(\.fromAddress))
            for s in senders { try? mail.setSenderRule(s, allow: true) }
        }
        run(uids) { [mail] f, u, a in try await mail.spam(f, u, toSpam: toSpam, on: a) }
    }

    /// Adds or removes a Gmail label on the checked messages (the selection stays).
    func setLabel(_ label: String, _ on: Bool) {
        let uids = Array(selected)
        guard !uids.isEmpty else { return }
        let apply: (Bool) -> Void = { [weak self] add in
            self?.update(uids) { h in
                h.labels.removeAll { $0 == label }
                if add { h.labels.append(label) }
            }
        }
        apply(on)
        let folder = folderId
        let account = account
        Task {
            do {
                try await mail.setLabel(folder, uids, label, on, on: account)
            } catch is AccountChanged {
            } catch {
                apply(!on)
                notices.error(describe(error))
            }
        }
    }

    /// A message was opened: it is read now.
    func opened(_ uids: [UInt32]) { update(uids) { $0.seen = true } }
}
