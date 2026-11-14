import SwiftUI

/// The mailbox: folders · message list · reader. Three columns on iPad, a
/// stack of screens on iPhone (it starts on the message list).
struct MainView: View {
    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Preferences.self) private var prefs
    @Environment(Notices.self) private var notices
    @Environment(\.scenePhase) private var scenePhase

    @State private var model: MailboxModel?
    @State private var column: NavigationSplitViewColumn = .content
    @State private var sheet: Sheet?
    /// The app went to the background (not only inactive, as with Control
    /// Center): coming back refreshes the open list.
    @State private var wasInBackground = false

    enum Sheet: Identifiable {
        case compose(ComposeRequest)
        case settings, contacts, calendar, addAccount, pro(String?)
        var id: String {
            switch self {
            case .compose(let r): "compose\(r.id)"
            case .settings: "settings"
            case .contacts: "contacts"
            case .calendar: "calendar"
            case .addAccount: "add"
            case .pro(let wanted): "pro\(wanted ?? "")"
            }
        }
    }

    var body: some View {
        Group {
            if let model {
                split(model)
            } else {
                ProgressView()
            }
        }
        .task {
            if model == nil {
                model = MailboxModel(mail: mail, prefs: prefs, notices: notices)
            }
            await model?.start()
            #if DEBUG
            if let model {
                await PerfProbe.showcase(model)
                await PerfProbe.autotest(model)
            }
            #endif
        }
        // A notification was tapped (also before the mailbox was ready: the
        // request waits for it).
        .task(id: "\(app.pendingOpen?.email ?? "")|\(app.pendingOpen?.uid ?? 0)|\(model != nil)") {
            await openFromNotification()
        }
        // A Focus that does not show the account in use: one it shows.
        .task(id: mail.focus) {
            guard let first = mail.visibleAccounts.first,
                  !mail.visibleAccounts.contains(where: { $0.email == mail.activeEmail }) else { return }
            try? await mail.switchTo(first.email)
        }
        // A link or a shortcut asked for a new message.
        .task(id: "\(app.pendingCompose?.id.uuidString ?? "")|\(model != nil)") {
            guard let request = app.pendingCompose, model != nil else { return }
            app.pendingCompose = nil
            sheet = .compose(request)
        }
        // New mail (or mail read elsewhere) for this account while open.
        .onChange(of: mail.arrivals) { _, _ in
            Task { await model?.serverChanged() }
        }
        // Back on screen: changes made elsewhere meanwhile (mail deleted or
        // read on the computer) show without pulling to refresh.
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .background:
                wasInBackground = true
            case .active where wasInBackground:
                wasInBackground = false
                Task { await model?.returned() }
            default:
                break
            }
        }
        .onChange(of: mail.changes) { _, _ in
            Task { await model?.reloadLocal() }
        }
        .sheet(item: $sheet) { sheet in
            sheetContent(sheet)
                .showsNotices()
        }
    }

    @ViewBuilder
    private func sheetContent(_ sheet: Sheet) -> some View {
            switch sheet {
            case .compose(let request):
                ComposeView(request: request)
            case .settings:
                SettingsView(addAccount: { self.sheet = app.mayAddAccount ? .addAccount : .pro(nil) })
            case .contacts:
                ContactsView(write: { to in self.sheet = .compose(ComposeRequest(to: to)) })
            case .calendar:
                CalendarView()
            case .addAccount:
                NavigationStack { SetupView(onDone: { self.sheet = nil }) }
            case .pro(let wanted):
                ProOffer(onBought: {
                    self.sheet = nil
                    if let wanted {
                        Task { try? await mail.switchTo(wanted) }
                    } else {
                        Task { @MainActor in
                            try? await Task.sleep(for: .milliseconds(400))
                            self.sheet = .addAccount
                        }
                    }
                })
            }
    }

    /// Opens the message of a tapped notification: switches to its account
    /// first (this screen then starts over and comes back here), then its
    /// inbox and the message.
    private func openFromNotification() async {
        guard let pending = app.pendingOpen, let model else { return }
        if pending.email.caseInsensitiveCompare(mail.activeEmail ?? "") != .orderedSame {
            guard app.mayUse(pending.email) else {
                app.pendingOpen = nil
                return
            }
            try? await mail.switchTo(pending.email)
            return
        }
        app.pendingOpen = nil
        sheet = nil
        await model.start()
        if model.folderId != mail.inboxId { model.openFolder(mail.inboxId) }
        guard let uid = pending.uid else {
            column = .content
            return
        }
        if model.header(uid) == nil { await model.refresh() }
        guard let header = model.header(uid) else {
            column = .content
            return
        }
        model.open = Row(conversation: Conversation(latest: header, uids: [uid], unread: header.seen ? 0 : 1))
    }

    private func split(_ model: MailboxModel) -> some View {
        NavigationSplitView(preferredCompactColumn: $column) {
            Sidebar(model: model, sheet: $sheet, column: $column)
        } content: {
            MessageListView(model: model, compose: { sheet = .compose($0) })
        } detail: {
            NavigationStack {
                if let row = model.open {
                    DetailView(model: model, row: row, compose: { sheet = .compose($0) })
                        .id(row.id)
                } else {
                    ContentUnavailableView(t("select_message"), systemImage: "envelope.open")
                }
            }
        }
        .onChange(of: model.open) { _, row in
            // On iPhone: the reader opens, or closes back to the list.
            column = row == nil ? .content : .detail
        }
    }
}

/// What the detail column shows for a row: the message, or the conversation.
struct DetailView: View {
    let model: MailboxModel
    let row: Row
    let compose: (ComposeRequest) -> Void

    var body: some View {
        if row.uids.count > 1 {
            ConversationView(model: model, row: row, compose: compose)
        } else {
            MessageView(model: model, folderId: model.folderId, uid: row.id, starred: row.flagged, compose: compose)
        }
    }
}

/// Folders, accounts and the other parts of the app.
struct Sidebar: View {
    let model: MailboxModel
    @Binding var sheet: MainView.Sheet?
    @Binding var column: NavigationSplitViewColumn

    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices

    var body: some View {
        // Rows are buttons, not a List selection: tapping the folder that
        // is already open must also go back to its list (on iPhone).
        List {
            if !mail.accounts.isEmpty {
                Section(t("section_account")) {
                    ForEach(mail.visibleAccounts, id: \.email) { account in
                        Button {
                            use(account.email)
                        } label: {
                            HStack {
                                Avatar(name: account.displayName.isEmpty ? account.email : account.displayName, size: 28)
                                VStack(alignment: .leading) {
                                    Text(account.displayName.isEmpty ? account.email : account.displayName)
                                        .lineLimit(1)
                                    if !account.displayName.isEmpty {
                                        Text(account.email).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                                    }
                                }
                                Spacer()
                                if account.email == mail.activeEmail {
                                    Image(systemName: "checkmark").foregroundStyle(.tint)
                                } else if !app.mayUse(account.email) {
                                    Image(systemName: "lock").foregroundStyle(.secondary)
                                }
                                // Unread mail in its inbox (as on the app icon).
                                let unread = unreadCount(account.email)
                                if unread > 0 {
                                    Text("\(unread)")
                                        .font(.caption.bold().monospacedDigit())
                                        .foregroundStyle(.white)
                                        .padding(.horizontal, 7)
                                        .padding(.vertical, 2)
                                        .background(Capsule().fill(Color.accentColor))
                                        .accessibilityLabel(t("intent_unread_count", ["n": "\(unread)"]))
                                }
                            }
                        }
                        .foregroundStyle(.primary)
                    }
                    Button {
                        sheet = app.mayAddAccount ? .addAccount : .pro(nil)
                    } label: {
                        Label(t("add_account"), systemImage: "plus")
                    }
                }
            }

            Section(t("section_folders")) {
                // A tree that folds, as webmails show long lists: folders
                // with subfolders start closed.
                ForEach(folderTree(folders: mail.folders, expanded: Array(expanded)), id: \.folder.id) { node in
                    let folder = node.folder
                    HStack(spacing: 0) {
                        Button {
                            if folder.selectable {
                                model.openFolder(folder.id)
                                column = .content
                            } else {
                                toggle(folder.id)
                            }
                        } label: {
                            FolderRow(folder: folder, depth: node.depth, unread: unread(node))
                        }
                        .buttonStyle(.plain)
                        if node.hasChildren {
                            Button { toggle(folder.id) } label: {
                                Image(systemName: "chevron.right")
                                    .font(.footnote.weight(.semibold))
                                    .foregroundStyle(.secondary)
                                    .rotationEffect(.degrees(node.expanded ? 90 : 0))
                                    .frame(width: 32, height: 28)
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(folder.name)
                        }
                    }
                    .foregroundStyle(folder.id == model.folderId ? Color.accentColor : .primary)
                    .listRowBackground(folder.id == model.folderId ? Color.accentColor.opacity(0.14) : nil)
                }
            }

            Section {
                Button { sheet = .contacts } label: { Label(t("contacts"), systemImage: "person.2") }
                Button { sheet = .calendar } label: { Label(t("calendar"), systemImage: "calendar") }
                Button { sheet = .settings } label: { Label(t("settings"), systemImage: "gear") }
            }
            .foregroundStyle(.primary)
        }
        // The title (also the back button's) is drawn in the bar with the logo.
        .navigationTitle("Ferrowl")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { titleBar }
        .refreshable { await model.loadUnread() }
        .onAppear { expanded = Set(UserDefaults.standard.stringArray(forKey: expandedKey) ?? []) }
    }

    /// An account's unread count: the open inbox's for the account in use,
    /// the last one saved (notifications, background checks) for the others.
    private func unreadCount(_ email: String) -> Int {
        if email == mail.activeEmail, let n = model.unread[mail.inboxId], !Badge.isGmail(mail.activeAccount!) {
            return Int(n)
        }
        return Badge.count(for: email)
    }

    /// Open folders of the folder tree, remembered per account.
    @State private var expanded: Set<String> = []

    private var expandedKey: String { "expandedFolders:\(mail.activeEmail?.lowercased() ?? "")" }

    private func toggle(_ id: String) {
        withAnimation(.snappy) {
            if expanded.contains(id) { expanded.remove(id) } else { expanded.insert(id) }
        }
        UserDefaults.standard.set(Array(expanded), forKey: expandedKey)
    }

    /// A folder's unread count; a closed one adds its subfolders'.
    private func unread(_ node: FolderNode) -> UInt32 {
        let own = model.unread[node.folder.id] ?? 0
        guard node.hasChildren, !node.expanded else { return own }
        return node.descendants.reduce(own) { $0 + (model.unread[$1] ?? 0) }
    }

    /// "Ferrowl" with its logo at the start of the bar (no empty title row).
    @ToolbarContentBuilder
    private var titleBar: some ToolbarContent {
        ToolbarItem(placement: .principal) { Color.clear.frame(width: 1, height: 1) }
        if #available(iOS 26, *) {
            ToolbarItem(placement: .topBarLeading) { brand }
                // Plain, not on a glass button.
                .sharedBackgroundVisibility(.hidden)
        } else {
            ToolbarItem(placement: .topBarLeading) { brand }
        }
    }

    private var brand: some View {
        HStack(spacing: 10) {
            Image("Logo").resizable().frame(width: 36, height: 36)
            Text("Ferrowl").font(.title.bold())
        }
        .fixedSize()
        // A little room under the status bar.
        .padding(.top, 10)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }

    private func use(_ email: String) {
        guard email != mail.activeEmail else { return }
        guard app.mayUse(email) else {
            sheet = .pro(email)
            return
        }
        Task {
            do {
                try await mail.switchTo(email)
            } catch {
                notices.error(describe(error))
            }
        }
    }

}

extension Folder: Identifiable {}

struct FolderRow: View {
    let folder: Folder
    /// Nesting level in the folder tree (the path's, when not given).
    var depth: UInt32?
    let unread: UInt32

    var body: some View {
        HStack {
            Label(folder.kind == .other ? (folder.path.last ?? folder.name) : folder.name, systemImage: icon)
                .padding(.leading, CGFloat(depth ?? (folder.kind == .other ? UInt32(max(0, folder.path.count - 1)) : 0)) * 14)
            Spacer()
            if unread > 0 {
                Text("\(unread)")
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var icon: String {
        switch folder.kind {
        case .inbox: "tray"
        case .sent: "paperplane"
        case .drafts: "doc"
        case .archive: "archivebox"
        case .spam: "xmark.bin"
        case .trash: "trash"
        case .other: "folder"
        }
    }
}

/// Initials on a colored circle.
struct Avatar: View {
    let name: String
    var size: CGFloat = 40

    var body: some View {
        let palette: [Color] = [Theme.hex(0xA63A0C), Theme.hex(0x1E5AA8), Theme.hex(0x2F6B45),
                                Theme.hex(0x6B4FA0), Theme.hex(0x8A5A12), Theme.hex(0x3B78C9)]
        let index = abs(name.unicodeScalars.reduce(0) { ($0 &* 31) &+ Int($1.value) }) % palette.count
        Text(initials(name: name))
            .font(.system(size: size * 0.4, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(palette[index], in: Circle())
            .accessibilityHidden(true)
    }
}
