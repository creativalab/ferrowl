import SwiftUI

/// The message list of the open folder: pull to refresh, search, swipe
/// to archive / delete, checking several messages for bulk actions.
struct MessageListView: View {
    @Bindable var model: MailboxModel
    let compose: (ComposeRequest) -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(Preferences.self) private var prefs
    @State private var moving = false
    @State private var labeling = false
    @State private var confirmDelete = false

    var body: some View {
        let rows = model.rows
        List(selection: model.selecting ? .constant(nil) : $model.open) {
            if model.tabs {
                GmailTabs(model: model)
                    .listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
            }
            // Worked out once for the whole list, not for each row.
            let deletesForever = model.deletesForever
            let inSpam = model.inSpam
            // Not over the rows themselves: they compare by message (for a
            // selection that survives a change of state), so the list would
            // take a row that became read or unread for unchanged and keep
            // drawing the old state. ListRow decides what to redraw.
            ForEach(rows.map(ListItem.init)) { item in
                let row = item.row
                ListRow(row: row, model: model, threaded: prefs.threaded, selecting: model.selecting,
                        selected: model.selecting && model.isSelected(row),
                        deletesForever: deletesForever, inSpam: inSpam,
                        move: { moving = true }, confirmDelete: { confirmDelete = true })
                    // Drawn again only when the row changes: not for every
                    // tap, which changes the list's selection.
                    .equatable()
                    .tag(row)
            }
        }
        .listStyle(.plain)
        .overlay(alignment: .bottomTrailing) {
            if !model.selecting {
                ComposeButton { compose(ComposeRequest()) }
                    .padding(20)
            }
        }
        .overlay {
            if rows.isEmpty {
                if model.refreshing {
                    ProgressView(t("list_loading"))
                } else {
                    ContentUnavailableView(t("list_empty"), systemImage: "tray")
                }
            }
        }
        .refreshable { await model.refresh() }
        .searchable(text: $model.query, prompt: t("search_folder"))
        .searchSuggestions {
            // The search commands (as in Gmail, in English in every language).
            if model.query.isEmpty {
                ForEach(SearchCommand.all, id: \.text) { c in
                    Label {
                        HStack {
                            Text(c.text).font(.body.monospaced())
                            Text(t(c.labelKey)).foregroundStyle(.secondary)
                        }
                    } icon: {
                        Image(systemName: c.icon)
                    }
                    .searchCompletion(c.text)
                }
            }
        }
        .navigationTitle(model.folder?.name ?? t("folder_inbox"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { toolbar }
        .confirmationDialog(t("confirm_delete_title"), isPresented: $confirmDelete, titleVisibility: .visible) {
            Button(t("delete_forever"), role: .destructive) { model.delete() }
        } message: {
            Text(t("cannot_undo"))
        }
        .sheet(isPresented: $moving) {
            FolderPicker(exclude: model.folderId) { target in model.move(to: target) }
        }
        .sheet(isPresented: $labeling) {
            LabelPicker(model: model)
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        if model.selecting {
            ToolbarItem(placement: .cancellationAction) {
                Button(t("close")) { model.endSelection() }
            }
            ToolbarItem(placement: .principal) {
                Text(t("selected_count", ["n": "\(model.selected.count)"])).font(.headline)
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button(t("select_all")) { model.select(.all) }
                    Button(t("select_read")) { model.select(.read) }
                    Button(t("select_unread")) { model.select(.unread) }
                    Button(t("select_starred")) { model.select(.starred) }
                    Button(t("select_none")) { model.select(.none) }
                } label: {
                    Image(systemName: "checklist")
                }
                .accessibilityLabel(t("select"))
            }
            ToolbarItemGroup(placement: .bottomBar) {
                let none = model.selected.isEmpty
                Button { model.archive() } label: { Image(systemName: "archivebox") }
                    .accessibilityLabel(t("archive")).disabled(none)
                Spacer()
                Button { moving = true } label: { Image(systemName: "folder") }
                    .accessibilityLabel(t("move_to")).disabled(none)
                Spacer()
                Menu {
                    Button(t("mark_read")) { model.markRead(true) }
                    Button(t("mark_unread")) { model.markRead(false) }
                    Button(t("star_add")) { model.star(true) }
                    Button(t("star_remove")) { model.star(false) }
                    Button(t(model.inSpam ? "spam_not" : "spam_report")) { model.spam() }
                    if mail.isGmail { Button(t("labels")) { labeling = true } }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .disabled(none)
                Spacer()
                Button(role: .destructive) {
                    if model.deletesForever { confirmDelete = true } else { model.delete() }
                } label: { Image(systemName: "trash") }
                    .accessibilityLabel(t("delete")).disabled(none)
            }
        } else {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button { model.selecting = true } label: { Label(t("select"), systemImage: "checkmark.circle") }
                    Picker(t("list_view"), selection: Bindable(prefs).threaded) {
                        Text(t("view_threads")).tag(true)
                        Text(t("view_messages")).tag(false)
                    }
                    Picker(t("sort_by"), selection: Bindable(prefs).sort) {
                        ForEach(Preferences.orders, id: \.self) { order in
                            Text(t(Preferences.label(of: order))).tag(order)
                        }
                    }
                    .pickerStyle(.menu)
                } label: {
                    Image(systemName: "line.3.horizontal.decrease.circle")
                }
                .accessibilityLabel(t("sort_by"))
            }
        }
    }
}

/// One row: sender, time, subject, preview, unread dot, star, clip.
struct MessageRow: View {
    let row: Row
    let threaded: Bool

    @Environment(Preferences.self) private var prefs

    var body: some View {
        let h = row.header
        HStack(alignment: .top, spacing: 10) {
            Circle()
                .fill(row.unread ? Theme.current(prefs.theme).unreadDot : .clear)
                .frame(width: 8, height: 8)
                .padding(.top, 6)
            VStack(alignment: .leading, spacing: 2) {
                HStack(alignment: .firstTextBaseline) {
                    Text(h.from.isEmpty ? h.fromAddress : h.from)
                        .font(.body.weight(row.unread ? .semibold : .regular))
                        .lineLimit(1)
                    if threaded, row.uids.count > 1 {
                        Text("\(row.uids.count)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 8)
                    if h.hasAttachment {
                        Image(systemName: "paperclip").font(.caption).foregroundStyle(.secondary)
                    }
                    if row.flagged {
                        Image(systemName: "star.fill").font(.caption).foregroundStyle(.yellow)
                    }
                    Text(Dates.short(h.timestamp))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                Text(h.subject.isEmpty ? t("no_subject") : h.subject)
                    .font(.subheadline.weight(row.unread ? .semibold : .regular))
                    .lineLimit(1)
                if !h.preview.isEmpty {
                    Text(h.preview)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }
}

/// Gmail inbox tabs (Primary, Promotions, Social, Updates, Forums).
struct GmailTabs: View {
    @Bindable var model: MailboxModel
    private let keys = ["gmail_primary", "gmail_promotions", "gmail_social", "gmail_updates", "gmail_forums"]

    var body: some View {
        let unread = model.unreadByCategory
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(keys.indices, id: \.self) { i in
                    let count = unread[i] ?? 0
                    Button {
                        model.category = i
                        model.selected = []
                    } label: {
                        Text(count > 0 ? "\(t(keys[i])) · \(count)" : t(keys[i]))
                            .font(.subheadline)
                            .padding(.horizontal, 12)
                            .padding(.vertical, 6)
                            .background(model.category == i ? Color.accentColor.opacity(0.15) : Color(.secondarySystemFill),
                                        in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

/// Picks a folder to move messages to.
struct FolderPicker: View {
    let exclude: String
    let pick: (String) -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List(mail.folders.filter { $0.selectable && $0.id != exclude }) { folder in
                Button {
                    dismiss()
                    pick(folder.id)
                } label: {
                    FolderRow(folder: folder, unread: 0)
                }
                .foregroundStyle(.primary)
            }
            .navigationTitle(t("move_to"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(t("cancel")) { dismiss() } }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

/// Gmail labels of the checked messages, to add or remove.
struct LabelPicker: View {
    let model: MailboxModel
    @Environment(MailRepository.self) private var mail
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            let choices = mail.labelChoices(model.folderId)
            List {
                if choices.isEmpty { Text(t("labels_none")).foregroundStyle(.secondary) }
                ForEach(choices) { folder in
                    let on = model.selected.allSatisfy { uid in model.header(uid)?.labels.contains(folder.id) ?? false }
                    Button {
                        model.setLabel(folder.id, !on)
                    } label: {
                        HStack {
                            Text(folder.name)
                            Spacer()
                            if on { Image(systemName: "checkmark").foregroundStyle(.tint) }
                        }
                    }
                    .foregroundStyle(.primary)
                }
            }
            .navigationTitle(t("labels"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(t("close")) {
                        model.endSelection()
                        dismiss()
                    }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

/// "Write": a round button in the accent color at the bottom right, over
/// the list (as on Android and in the desktop's design).
struct ComposeButton: View {
    let action: () -> Void
    @Environment(Preferences.self) private var prefs

    var body: some View {
        Button(action: action) {
            Image(systemName: "pencil")
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 48, height: 48)
                .background(Theme.current(prefs.theme).accent, in: Circle())
                .shadow(color: .black.opacity(0.22), radius: 6, y: 3)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(t("compose"))
    }
}

/// A search command offered when the search opens.
struct SearchCommand {
    let text: String
    let labelKey: String
    let icon: String

    static let all = [
        SearchCommand(text: "from:", labelKey: "from", icon: "person"),
        SearchCommand(text: "has:attachment ", labelKey: "filter_attachments", icon: "paperclip"),
        SearchCommand(text: "is:unread ", labelKey: "filter_unread", icon: "envelope.badge"),
        SearchCommand(text: "is:starred ", labelKey: "filter_starred", icon: "star"),
    ]
}

/// A row as the list iterates it: identified by its message, never equal
/// to another state of itself (see `ForEach` above).
private struct ListItem: Identifiable {
    let row: Row
    var id: UInt32 { row.id }
}

/// A row of the message list, with its swipe actions and menu. Equatable on
/// what it shows, so SwiftUI skips rows that did not change.
private struct ListRow: View, Equatable {
    let row: Row
    let model: MailboxModel
    let threaded: Bool
    let selecting: Bool
    let selected: Bool
    let deletesForever: Bool
    let inSpam: Bool
    let move: () -> Void
    let confirmDelete: () -> Void

    nonisolated static func == (a: ListRow, b: ListRow) -> Bool {
        a.row.conversation == b.row.conversation && a.threaded == b.threaded && a.selecting == b.selecting && a.selected == b.selected
            && a.deletesForever == b.deletesForever && a.inSpam == b.inSpam
    }

    var body: some View {
        if selecting {
            Button {
                model.toggle(row)
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                        .font(.title3)
                        .foregroundStyle(selected ? Color.accentColor : .secondary)
                    MessageRow(row: row, threaded: threaded)
                }
            }
            .foregroundStyle(.primary)
        } else {
            MessageRow(row: row, threaded: threaded)
                .swipeActions(edge: .leading, allowsFullSwipe: true) {
                    Button {
                        model.markRead(row.unread, row)
                    } label: {
                        Label(t(row.unread ? "mark_read" : "mark_unread"),
                              systemImage: row.unread ? "envelope.open" : "envelope.badge")
                    }
                    .tint(.blue)
                    Button {
                        model.star(!row.flagged, row)
                    } label: {
                        Label(t(row.flagged ? "star_remove" : "star_add"), systemImage: row.flagged ? "star.slash" : "star")
                    }
                    .tint(.yellow)
                }
                // To the left: delete (to the Trash; not in the Trash itself,
                // where it would be for good without asking).
                .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                    if !deletesForever {
                        Button(role: .destructive) {
                            model.deleteWithUndo(row)
                        } label: {
                            Label(t("delete"), systemImage: "trash")
                        }
                    }
                }
                .contextMenu {
                    Button { model.markRead(row.unread, row) } label: {
                        Label(t(row.unread ? "mark_read" : "mark_unread"), systemImage: "envelope")
                    }
                    Button { model.star(!row.flagged, row) } label: {
                        Label(t(row.flagged ? "star_remove" : "star_add"), systemImage: "star")
                    }
                    Button { model.archive(row) } label: { Label(t("archive"), systemImage: "archivebox") }
                    Button { model.spam(row) } label: {
                        Label(t(inSpam ? "spam_not" : "spam_report"), systemImage: "xmark.bin")
                    }
                    Button {
                        model.selected = Set(row.uids)
                        move()
                    } label: { Label(t("move_to"), systemImage: "folder") }
                    Button(role: .destructive) {
                        if deletesForever {
                            model.selected = Set(row.uids)
                            confirmDelete()
                        } else {
                            model.deleteWithUndo(row)
                        }
                    } label: { Label(t(deletesForever ? "delete_forever" : "delete"), systemImage: "trash") }
                }
        }
    }
}
