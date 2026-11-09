import SwiftUI

/// The mailbox: folders · message list · reader. Three columns on iPad, a
/// stack of screens on iPhone (it starts on the message list).
struct MainView: View {
    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Preferences.self) private var prefs
    @Environment(Notices.self) private var notices

    @State private var model: MailboxModel?
    @State private var column: NavigationSplitViewColumn = .content
    @State private var sheet: Sheet?

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
                    ForEach(mail.accounts, id: \.email) { account in
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
                ForEach(ordered) { folder in
                    Button {
                        model.openFolder(folder.id)
                        column = .content
                    } label: {
                        FolderRow(folder: folder, unread: model.unread[folder.id] ?? 0)
                    }
                    .foregroundStyle(folder.id == model.folderId ? Color.accentColor : .primary)
                    .listRowBackground(folder.id == model.folderId ? Color.accentColor.opacity(0.14) : nil)
                    .disabled(!folder.selectable)
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

    /// Standard folders first (Inbox, Sent, …), then the others by path.
    private var ordered: [Folder] {
        let rank: (FolderKind) -> Int = {
            switch $0 {
            case .inbox: 0
            case .drafts: 1
            case .sent: 2
            case .archive: 3
            case .spam: 4
            case .trash: 5
            case .other: 6
            }
        }
        return mail.folders.sorted {
            let a = rank($0.kind), b = rank($1.kind)
            if a != b { return a < b }
            return $0.path.joined(separator: "/").localizedStandardCompare($1.path.joined(separator: "/")) == .orderedAscending
        }
    }
}

extension Folder: Identifiable {}

struct FolderRow: View {
    let folder: Folder
    let unread: UInt32

    var body: some View {
        HStack {
            Label(folder.kind == .other ? (folder.path.last ?? folder.name) : folder.name, systemImage: icon)
                .padding(.leading, folder.kind == .other ? CGFloat(max(0, folder.path.count - 1)) * 14 : 0)
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
