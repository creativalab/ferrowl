import QuickLook
import SwiftUI

/// What opens the composer: a new message (maybe to someone), or a
/// reply / forward of a message.
struct ComposeRequest: Identifiable {
    enum Kind { case new, reply, replyAll, forward }
    let id = UUID()
    var kind: Kind = .new
    var to = ""
    var message: Message?
    /// A draft asked for by a link or a shortcut (new messages only).
    var cc = ""
    var subject = ""
    var body = ""
    /// Files shared to Ferrowl from another app, to attach.
    var files: [URL] = []
}

/// An opened message, as `MessageViewModel` on Android.
@MainActor
@Observable
final class MessageModel {
    enum State {
        case loading
        case shown(Message)
        case failed(String)
    }

    let mail: MailRepository
    let notices: Notices
    let folderId: String
    let uid: UInt32
    /// The message as listed, shown while the whole message loads.
    let listed: Header?
    private(set) var state: State = .loading
    /// The HTML loads images from the network (worked out once, off the
    /// main thread: the search is slow on big newsletters).
    private(set) var remoteImagesInside = false
    /// What the message is about (a shipment, a code, a bill).
    private(set) var insights: [MailInsight] = []
    var starred: Bool
    /// Our answer to the message's invitation, and whether a cancelled
    /// event is still in the calendar.
    private(set) var inviteReply: InviteReply?
    private(set) var inCalendar = false
    private(set) var inviteBusy = false

    init(mail: MailRepository, notices: Notices, folderId: String, uid: UInt32, starred: Bool, listed: Header? = nil) {
        self.mail = mail
        self.notices = notices
        self.folderId = folderId
        self.uid = uid
        self.starred = starred
        self.listed = listed
    }

    var message: Message? {
        if case .shown(let m) = state { return m }
        return nil
    }

    func load() async {
        guard case .loading = state else { return }
        do {
            let message = try await mail.message(folderId, uid)
            if let html = message.html {
                remoteImagesInside = await Task.detached(priority: .userInitiated) {
                    MessageBody.hasRemoteImages(html)
                }.value
            }
            insights = await Task.detached(priority: .userInitiated) { mailInsights(message: message) }.value
            state = .shown(message)
            refreshInvite()
        } catch {
            state = .failed(describe(error))
        }
    }

    private func refreshInvite() {
        guard let inv = message?.invitation else { return }
        inviteReply = mail.invitationReply(inv.uid)
        inCalendar = inv.method == "CANCEL" && mail.inCalendar(inv.uid)
    }

    func respond(_ reply: InviteReply) {
        guard let calendar = message?.calendar, !inviteBusy else { return }
        inviteBusy = true
        Task {
            defer { inviteBusy = false }
            do {
                notices.info(try await mail.respondInvitation(calendar, reply))
            } catch {
                notices.error(describe(error))
            }
            refreshInvite()
        }
    }

    func removeCancelled() {
        guard let uid = message?.invitation?.uid else { return }
        inviteBusy = true
        Task {
            defer { inviteBusy = false }
            do {
                notices.info(try await mail.removeCancelled(uid))
            } catch {
                notices.error(describe(error))
            }
            refreshInvite()
        }
    }

    var deletesForever: Bool { mail.deletesForever(folderId) }
    var inSpam: Bool { mail.isSpam(folderId) }

    /// Runs an action that takes the message away; true when done.
    func act(_ action: @escaping () async throws -> Void) async -> Bool {
        do {
            try await action()
            return true
        } catch {
            notices.error(describe(error))
            return false
        }
    }

    func toggleStar() {
        let on = !starred
        starred = on
        Task {
            do {
                try await mail.setFlagged(folderId, [uid], on)
            } catch {
                starred = !on
                notices.error(describe(error))
            }
        }
    }

    /// Blocks the sender: the address goes to the blocked list, the message to Spam.
    func blockSender() async -> Bool {
        guard let from = message?.fromAddress, !from.isEmpty else { return false }
        return await act { [self] in
            try mail.setSenderRule(from, allow: false)
            notices.info(t("sender_blocked", ["who": from]))
            if !inSpam { try await mail.spam(folderId, [uid], toSpam: true) }
        }
    }

    /// Report spam, or in Spam "not spam" (which also allows the sender).
    func spam() async -> Bool {
        let toSpam = !inSpam
        let from = message?.fromAddress ?? ""
        return await act { [self] in
            if !toSpam, !from.isEmpty { try mail.setSenderRule(from, allow: true) }
            try await mail.spam(folderId, [uid], toSpam: toSpam)
        }
    }
}

struct MessageView: View {
    let mailbox: MailboxModel?
    let compose: (ComposeRequest) -> Void
    /// Inside a conversation the actions stay on the conversation screen.
    var embedded = false

    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss
    @State private var model: MessageModel
    @State private var confirmDelete = false
    @State private var moving = false

    init(model mailbox: MailboxModel?, folderId: String, uid: UInt32, starred: Bool,
         compose: @escaping (ComposeRequest) -> Void, embedded: Bool = false, mail: MailRepository? = nil) {
        self.mailbox = mailbox
        self.compose = compose
        self.embedded = embedded
        let repo = mail ?? mailbox!.mail
        _model = State(initialValue: MessageModel(mail: repo, notices: repo.notices, folderId: folderId, uid: uid,
                                                  starred: starred, listed: mailbox?.header(uid)))
    }

    var body: some View {
        // One scroll view whatever the state: only its content changes when
        // the message arrives, so the screen is not replaced mid-transition.
        ScrollView {
            switch model.state {
            case .loading:
                if let listed = model.listed {
                    // What the list already knows, at once; the rest follows.
                    ListedHeader(header: listed).padding()
                } else {
                    ProgressView(t("loading_message"))
                        .frame(maxWidth: .infinity)
                        .padding(.top, 120)
                }
            case .failed(let error):
                ContentUnavailableView(t("err_generic"), systemImage: "exclamationmark.triangle", description: Text(error))
                    .padding(.top, 60)
            case .shown(let message):
                MessageBody(message: message, model: model, compose: compose)
                    .padding()
                    // Room for the actions floating at the bottom.
                    .padding(.bottom, embedded ? 0 : 68)
            }
        }
        // Opaque: nothing of the list shows through, even mid-transition.
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(.systemBackground))
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await model.load()
            mailbox?.opened([model.uid])
            // Read now: the unread counts (and the app icon's number) change.
            await mailbox?.loadUnread()
        }
        .toolbar { if !embedded { toolbar } }
        .overlay(alignment: .bottomLeading) {
            if !embedded, model.message != nil { actions }
        }
        .confirmationDialog(t("confirm_delete_title"), isPresented: $confirmDelete, titleVisibility: .visible) {
            Button(t("delete_forever"), role: .destructive) { close { try await mail.delete(model.folderId, [model.uid]) } }
        } message: {
            Text(t("cannot_undo"))
        }
        .sheet(isPresented: $moving) {
            FolderPicker(exclude: model.folderId) { target in close { try await mail.move(model.folderId, [model.uid], to: target) } }
        }
    }

    /// Runs an action that takes the message away, then closes it.
    private func close(_ action: @escaping () async throws -> Void) {
        Task {
            if await model.act(action) { leave() }
        }
    }

    private func leave() {
        // Back to the message list, also from inside a conversation.
        if let mailbox {
            mailbox.open = nil
        } else {
            dismiss()
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Button { model.toggleStar() } label: {
                Image(systemName: model.starred ? "star.fill" : "star")
            }
            .accessibilityLabel(t(model.starred ? "star_remove" : "star_add"))
            // At the top, as on Android: away from the reply buttons.
            Button(action: trash) { Image(systemName: "trash") }
                .accessibilityLabel(t("delete"))
            Menu {
                Button { close { try await mail.setSeen(model.folderId, [model.uid], false) } } label: {
                    Label(t("mark_unread"), systemImage: "envelope.badge")
                }
                Button { moving = true } label: { Label(t("move_to"), systemImage: "folder") }
                Button { Task { if await model.spam() { leave() } } } label: {
                    Label(t(model.inSpam ? "spam_not" : "spam_report"), systemImage: "xmark.bin")
                }
                Button { Task { if await model.blockSender() { leave() } } } label: {
                    Label(t("block_sender"), systemImage: "hand.raised")
                }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
        }
    }

    /// To the Trash (in the Trash: for good, after asking).
    private func trash() {
        if model.deletesForever {
            confirmDelete = true
        } else if let mailbox, let header = mailbox.header(model.uid) {
            // As a swipe in the list: "Moved to Trash · Undo".
            let unread: UInt32 = header.seen ? 0 : 1
            mailbox.deleteWithUndo(Row(conversation: Conversation(latest: header, uids: [model.uid], unread: unread)))
            leave()
        } else {
            close { try await mail.delete(model.folderId, [model.uid]) }
        }
    }

    /// Archive, reply, reply all, forward: small, at the bottom left, on a frosted
    /// pill that stands out over the message.
    private var actions: some View {
        HStack(spacing: 2) {
            ActionIcon(name: "archivebox", label: t("archive")) {
                close { try await mail.archive(model.folderId, [model.uid]) }
            }
            ActionIcon(name: "arrowshape.turn.up.left", label: t("reply")) { reply(.reply) }
            ActionIcon(name: "arrowshape.turn.up.left.2", label: t("reply_all")) { reply(.replyAll) }
            ActionIcon(name: "arrowshape.turn.up.right", label: t("forward")) { reply(.forward) }
        }
        .padding(.horizontal, 4)
        .background(.thickMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.primary.opacity(0.08)))
        .shadow(color: .black.opacity(0.18), radius: 8, y: 2)
        .padding(.leading, 28)
        .padding(.bottom, 12)
    }

    private func reply(_ kind: ComposeRequest.Kind) {
        guard let message = model.message else { return }
        compose(ComposeRequest(kind: kind, message: message))
    }
}

/// Subject, sender and date from the message list, with a spinner for the
/// body: shown at once while the message loads.
struct ListedHeader: View {
    let header: Header

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(header.subject.isEmpty ? t("no_subject") : header.subject)
                .font(.title2.weight(.semibold))
            HStack(alignment: .top, spacing: 12) {
                Avatar(name: header.from.isEmpty ? header.fromAddress : header.from)
                VStack(alignment: .leading, spacing: 2) {
                    Text(header.from.isEmpty ? header.fromAddress : header.from).font(.headline)
                    if !header.from.isEmpty {
                        Text(header.fromAddress).font(.footnote).foregroundStyle(.secondary)
                    }
                    Text(Dates.full(header.timestamp)).font(.footnote).foregroundStyle(.secondary)
                }
            }
            ProgressView(t("loading_message"))
                .frame(maxWidth: .infinity)
                .padding(.top, 32)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// Header, badges, invitation, body and attachments of a message.
struct MessageBody: View {
    let message: Message
    let model: MessageModel
    let compose: (ComposeRequest) -> Void

    @Environment(Preferences.self) private var prefs
    @Environment(\.colorScheme) private var scheme
    @State private var showText = false
    @State private var loadImages = false
    @State private var height: CGFloat = 120
    @State private var rendered = false
    @State private var preview: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(message.subject.isEmpty ? t("no_subject") : message.subject)
                .font(.title2.weight(.semibold))
                .textSelection(.enabled)

            HStack(alignment: .top, spacing: 12) {
                Avatar(name: message.from.isEmpty ? message.fromAddress : message.from)
                VStack(alignment: .leading, spacing: 2) {
                    Text(message.from.isEmpty ? message.fromAddress : message.from).font(.headline)
                    if !message.from.isEmpty {
                        Text(message.fromAddress).font(.footnote).foregroundStyle(.secondary)
                    }
                    if !message.to.isEmpty {
                        Text("\(t("to")): \(message.to)").font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                    }
                    if !message.cc.isEmpty {
                        Text("\(t("cc")): \(message.cc)").font(.footnote).foregroundStyle(.secondary).lineLimit(2)
                    }
                    Text(Dates.full(message.timestamp)).font(.footnote).foregroundStyle(.secondary)
                }
                .textSelection(.enabled)
            }

            badges

            if let invitation = message.invitation {
                InvitationCard(invitation: invitation, model: model)
            }
            InsightCards(insights: model.insights)

            if let html = message.html, !showText {
                if !prefs.remoteImages && !loadImages && model.remoteImagesInside {
                    HStack {
                        Text(t("images_blocked")).font(.footnote).foregroundStyle(.secondary)
                        Spacer()
                        Button(t("load_images")) { loadImages = true }.font(.footnote)
                    }
                }
                HTMLView(html: html, inline: message.inline, remoteImages: prefs.remoteImages || loadImages,
                         dark: scheme == .dark, height: $height, rendered: $rendered, openMailto: mailto)
                    .frame(height: height)
                    .overlay { if !rendered { ProgressView() } }
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                Button(t("view_text")) { showText = true }.font(.footnote)
            } else {
                Text(message.text.isEmpty ? AttributedString(t("no_text")) : Self.linked(message.text))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .environment(\.openURL, OpenURLAction { url in
                        if url.scheme?.lowercased() == "mailto" {
                            mailto(url)
                            return .handled
                        }
                        return .systemAction
                    })
                if message.html != nil {
                    Button(t("view_html")) { showText = false }.font(.footnote)
                }
            }

            if !message.attachments.isEmpty {
                attachments
            }
        }
        .quickLookPreview($preview)
    }

    @ViewBuilder
    private var badges: some View {
        let items = badgeItems
        if !items.isEmpty {
            HStack(spacing: 8) {
                ForEach(items, id: \.0) { item in
                    Label(item.0, systemImage: item.1)
                        .font(.caption.weight(.medium))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .foregroundStyle(item.2 ? Color.green : Color.orange)
                        .background((item.2 ? Color.green : Color.orange).opacity(0.12), in: Capsule())
                }
            }
        }
    }

    private var badgeItems: [(String, String, Bool)] {
        var out: [(String, String, Bool)] = []
        if let pgp = message.pgp {
            if pgp.error != nil {
                out.append((t("badge_cannot_decrypt"), "lock.slash", false))
            } else if pgp.encrypted {
                out.append((t("badge_encrypted"), "lock.fill", true))
            }
            switch pgp.signature {
            case .valid: out.append((t("badge_signed"), "signature", true))
            case .invalid: out.append((t("badge_sig_invalid"), "xmark.seal", false))
            case .unknownKey: out.append((t("badge_sig_unknown"), "questionmark.diamond", false))
            case .unverified: out.append((t("badge_sig_unverified"), "questionmark.diamond", false))
            case .keyChanged: out.append((t("badge_sig_key_changed"), "exclamationmark.triangle", false))
            case nil: break
            }
        }
        return out
    }

    private var attachments: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(message.attachments.count == 1 ? t("attachments_one") : t("attachments_many", ["n": "\(message.attachments.count)"]))
                .font(.subheadline.weight(.semibold))
            ForEach(message.attachments.indices, id: \.self) { i in
                let a = message.attachments[i]
                Button {
                    preview = Attachments.file(a, message: message.uid)
                } label: {
                    HStack {
                        Image(systemName: Attachments.icon(a.name))
                            .frame(width: 28)
                        VStack(alignment: .leading) {
                            Text(a.name).lineLimit(1)
                            Text(ByteCountFormatter.string(fromByteCount: Int64(a.data.count), countStyle: .file))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Image(systemName: "eye").foregroundStyle(.secondary)
                    }
                    .padding(10)
                    .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 10))
                }
                .foregroundStyle(.primary)
                .accessibilityLabel("\(t("preview_attachment")): \(a.name)")
            }
        }
    }

    /// A mailto: link in a message, with its subject and text if it has them.
    private func mailto(_ url: URL) {
        if case .compose(let to, let cc, _, let subject, let body) = parseLink(url: url.absoluteString) {
            compose(ComposeRequest(to: to, cc: cc, subject: subject, body: body))
        }
    }

    /// Plain text with its web addresses, e-mail addresses and phone numbers
    /// made into links.
    static func linked(_ text: String) -> AttributedString {
        var out = AttributedString(text)
        let types: NSTextCheckingResult.CheckingType = [.link, .phoneNumber]
        guard let detector = try? NSDataDetector(types: types.rawValue) else { return out }
        for match in detector.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
            let url: URL?
            if match.resultType == .phoneNumber, let number = match.phoneNumber {
                url = URL(string: "tel:" + number.filter { $0.isNumber || $0 == "+" })
            } else {
                url = match.url
            }
            guard let url, let range = Range(match.range, in: text),
                  let lower = AttributedString.Index(range.lowerBound, within: out),
                  let upper = AttributedString.Index(range.upperBound, within: out)
            else { continue }
            out[lower..<upper].link = url
        }
        return out
    }

    nonisolated static func hasRemoteImages(_ html: String) -> Bool {
        html.range(of: #"<img[^>]+src\s*=\s*["']?https?:"#, options: [.regularExpression, .caseInsensitive]) != nil
            || html.range(of: #"url\(\s*["']?https?:"#, options: [.regularExpression, .caseInsensitive]) != nil
    }
}

/// Attachments are written to the app's temporary folder to be previewed
/// (Quick Look, which also offers Share and Save to Files) or sent again.
enum Attachments {
    static func file(_ a: Attachment, message uid: UInt32) -> URL? {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("attachments/\(uid)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let safe = a.name.replacingOccurrences(of: "/", with: "_").replacingOccurrences(of: ":", with: "_")
        let url = dir.appendingPathComponent(safe.isEmpty ? "attachment" : safe)
        do {
            try a.data.write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }

    static func icon(_ name: String) -> String {
        switch (name as NSString).pathExtension.lowercased() {
        case "jpg", "jpeg", "png", "gif", "heic", "webp": "photo"
        case "pdf": "doc.richtext"
        case "zip", "gz", "7z", "rar": "doc.zipper"
        case "txt", "md", "csv": "doc.text"
        case "ics": "calendar"
        case "asc", "gpg", "pgp": "key"
        default: "doc"
        }
    }
}

/// A small icon button of the message actions.
struct ActionIcon: View {
    let name: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) { Self.glyph(name) }
            .accessibilityLabel(label)
    }

    static func glyph(_ name: String) -> some View {
        Image(systemName: name)
            .font(.system(size: 19, weight: .medium))
            .foregroundStyle(.primary)
            .frame(width: 48, height: 48)
            .contentShape(Rectangle())
    }
}
