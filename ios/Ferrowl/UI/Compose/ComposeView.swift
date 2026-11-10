import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// The composer's state, as `ComposeViewModel` on Android.
@MainActor
@Observable
final class ComposeModel {
    enum Field { case to, cc }

    struct File: Identifiable, Hashable {
        var id: String { path }
        let path: String
        let name: String
        let size: Int
    }

    let mail: MailRepository
    var to = "" { didSet { if to != oldValue { suggest(.to, to) } } }
    var cc = "" { didSet { if cc != oldValue { suggest(.cc, cc) } } }
    var bcc = ""
    var subject = ""
    var body = ""
    var showCc = false
    private(set) var attachments: [File] = []
    private(set) var suggestions: [AddressSuggestion] = []
    private(set) var suggestFor: Field?
    private(set) var sending = false
    private var draft: Outgoing
    private var suggestTask: Task<Void, Never>?
    /// Folder of this composer's copies of attached files.
    private let folder = FileManager.default.temporaryDirectory
        .appendingPathComponent("outgoing/\(UUID().uuidString)", isDirectory: true)

    init(mail: MailRepository, request: ComposeRequest) {
        self.mail = mail
        let own = mail.activeEmail ?? ""
        switch (request.kind, request.message) {
        case (.reply, let m?): draft = replyDraft(message: m, replyAll: false, ownAddress: own)
        case (.replyAll, let m?): draft = replyDraft(message: m, replyAll: true, ownAddress: own)
        case (.forward, let m?): draft = forwardDraft(message: m)
        default:
            draft = Outgoing(to: request.to, cc: request.cc, bcc: "", subject: request.subject, body: request.body,
                             html: nil, inReplyTo: nil,
                             references: [], attachments: [], sign: false, encrypt: false)
        }
        to = draft.to
        cc = draft.cc
        subject = draft.subject
        body = draft.body
        showCc = !cc.isEmpty
        // A forward carries the original attachments, as on desktop.
        if request.kind == .forward, let m = request.message {
            for a in m.attachments { add(name: a.name, data: a.data) }
        }
        // Files shared from another app ("Open in Ferrowl"): attached, and
        // the copy iOS made for us in Documents/Inbox is removed.
        for url in request.files {
            _ = add(file: url)
            if url.path.contains("/Documents/Inbox/") { try? FileManager.default.removeItem(at: url) }
        }
        suggestFor = nil
        suggestions = []
    }

    var canSend: Bool { !sending && to.contains("@") }

    var edited: Bool {
        to != draft.to || cc != draft.cc || !bcc.isEmpty || subject != draft.subject || body != draft.body
    }

    private func suggest(_ field: Field, _ text: String) {
        suggestTask?.cancel()
        let word = text.split(separator: ",", omittingEmptySubsequences: false).last
            .map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
        guard word.count >= 2 else {
            suggestions = []
            suggestFor = nil
            return
        }
        suggestTask = Task {
            let found = await mail.suggest(word)
            guard !Task.isCancelled else { return }
            // Nothing new to offer when the address is already complete.
            let useful = found.filter { $0.address.caseInsensitiveCompare(word) != .orderedSame }
            suggestions = Array(useful.prefix(6))
            suggestFor = useful.isEmpty ? nil : field
        }
    }

    /// Replaces the word being typed with the picked address.
    func pick(_ s: AddressSuggestion) {
        func replace(_ field: String) -> String {
            var parts = field.split(separator: ",", omittingEmptySubsequences: false).map { $0.trimmingCharacters(in: .whitespaces) }
            if !parts.isEmpty { parts.removeLast() }
            parts.append(s.address)
            return parts.filter { !$0.isEmpty }.joined(separator: ", ") + ", "
        }
        let field = suggestFor
        suggestFor = nil
        suggestions = []
        switch field {
        case .to: to = replace(to)
        case .cc: cc = replace(cc)
        case nil: break
        }
        suggestTask?.cancel()
        suggestions = []
        suggestFor = nil
    }

    func add(name: String, data: Data) {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        var url = folder.appendingPathComponent(name.isEmpty ? "attachment" : name)
        var n = 2
        while FileManager.default.fileExists(atPath: url.path) {
            let base = (name as NSString).deletingPathExtension
            let ext = (name as NSString).pathExtension
            url = folder.appendingPathComponent(ext.isEmpty ? "\(base) \(n)" : "\(base) \(n).\(ext)")
            n += 1
        }
        guard (try? data.write(to: url)) != nil else { return }
        attachments.append(File(path: url.path, name: url.lastPathComponent, size: data.count))
    }

    /// A file picked in Files (security-scoped: copied into the app first).
    func add(file url: URL) -> Bool {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let data = try? Data(contentsOf: url) else { return false }
        add(name: url.lastPathComponent, data: data)
        return true
    }

    func remove(_ file: File) {
        try? FileManager.default.removeItem(atPath: file.path)
        attachments.removeAll { $0 == file }
    }

    func discardFiles() {
        try? FileManager.default.removeItem(at: folder)
    }

    func send() async throws {
        sending = true
        defer { sending = false }
        var out = draft
        out.to = to.trimmingCharacters(in: CharacterSet(charactersIn: ", ").union(.whitespaces))
        out.cc = cc.trimmingCharacters(in: CharacterSet(charactersIn: ", ").union(.whitespaces))
        out.bcc = bcc.trimmingCharacters(in: CharacterSet(charactersIn: ", ").union(.whitespaces))
        out.subject = subject
        out.body = body
        out.attachments = attachments.map(\.path)
        try await mail.send(out)
        discardFiles()
    }
}

struct ComposeView: View {
    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss

    let request: ComposeRequest
    @State private var model: ComposeModel?
    @State private var confirmDiscard = false
    @State private var importing = false
    @State private var photos: [PhotosPickerItem] = []
    @State private var choosingPhotos = false
    @State private var shooting = false

    var body: some View {
        NavigationStack {
            if let model {
                form(model)
            }
        }
        .interactiveDismissDisabled(model?.edited ?? false)
        .task {
            if model == nil { model = ComposeModel(mail: mail, request: request) }
        }
    }

    private func form(_ model: ComposeModel) -> some View {
        @Bindable var model = model
        return Form {
            Section {
                recipient(t("to"), text: $model.to, field: .to, model: model)
                if model.showCc {
                    recipient(t("cc"), text: $model.cc, field: .cc, model: model)
                    recipient(t("bcc"), text: $model.bcc, field: nil, model: model)
                } else {
                    Button(t("cc_bcc")) { model.showCc = true }.font(.subheadline)
                }
                TextField(t("subject"), text: $model.subject)
            }
            Section {
                TextEditor(text: $model.body)
                    .frame(minHeight: 260)
                    .overlay(alignment: .topLeading) {
                        if model.body.isEmpty {
                            Text(t("body_placeholder")).foregroundStyle(.tertiary).padding(.top, 8).padding(.leading, 4)
                                .allowsHitTesting(false)
                        }
                    }
            }
            if !model.attachments.isEmpty {
                Section(model.attachments.count == 1 ? t("attachments_one") : t("attachments_many", ["n": "\(model.attachments.count)"])) {
                    ForEach(model.attachments) { file in
                        HStack {
                            Image(systemName: Attachments.icon(file.name))
                            Text(file.name).lineLimit(1)
                            Spacer()
                            Text(ByteCountFormatter.string(fromByteCount: Int64(file.size), countStyle: .file))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        .swipeActions {
                            Button(t("remove_attachment"), role: .destructive) { model.remove(file) }
                        }
                    }
                }
            }
        }
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button(t("cancel")) {
                    if model.edited { confirmDiscard = true } else { close(model) }
                }
            }
            ToolbarItem(placement: .confirmationAction) {
                if model.sending {
                    ProgressView()
                } else {
                    Button(t("send")) { send(model) }.disabled(!model.canSend)
                }
            }
            // In the top bar: a bottom bar would be hidden by the keyboard.
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button { importing = true } label: { Label(t("attach_file"), systemImage: "doc") }
                    Button { choosingPhotos = true } label: { Label(t("attach_photo"), systemImage: "photo.on.rectangle") }
                    if UIImagePickerController.isSourceTypeAvailable(.camera) {
                        Button { shooting = true } label: { Label(t("attach_camera"), systemImage: "camera") }
                    }
                } label: {
                    Image(systemName: "paperclip")
                }
                .accessibilityLabel(t("attach"))
            }
        }
        .confirmationDialog(t("discard"), isPresented: $confirmDiscard) {
            Button(t("discard"), role: .destructive) { close(model) }
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result {
                for url in urls where !model.add(file: url) {
                    notices.error(t("err_attach_read", ["path": url.lastPathComponent, "err": ""]))
                }
            }
        }
        .fullScreenCover(isPresented: $shooting) {
            CameraPicker { data in
                let stamp = Date().formatted(.iso8601.year().month().day().time(includingFractionalSeconds: false))
                    .replacingOccurrences(of: ":", with: "")
                model.add(name: "IMG_\(stamp).jpg", data: data)
            }
            .ignoresSafeArea()
        }
        .photosPicker(isPresented: $choosingPhotos, selection: $photos, matching: .any(of: [.images, .videos]))
        .onChange(of: photos) { _, items in
            guard !items.isEmpty else { return }
            photos = []
            Task {
                for (i, item) in items.enumerated() {
                    guard let data = try? await item.loadTransferable(type: Data.self) else { continue }
                    let ext = item.supportedContentTypes.first?.preferredFilenameExtension ?? "jpg"
                    model.add(name: "IMG_\(i + 1).\(ext)", data: data)
                }
            }
        }
        .disabled(model.sending)
    }

    @ViewBuilder
    private func recipient(_ label: String, text: Binding<String>, field: ComposeModel.Field?, model: ComposeModel) -> some View {
        LabeledContent(label) {
            TextField(label, text: text, prompt: Text(""))
                .keyboardType(.emailAddress)
                .textContentType(.emailAddress)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
        if let field, model.suggestFor == field {
            ForEach(model.suggestions, id: \.address) { s in
                Button {
                    model.pick(s)
                } label: {
                    VStack(alignment: .leading) {
                        Text(s.name.isEmpty ? s.address : s.name)
                        if !s.name.isEmpty { Text(s.address).font(.caption).foregroundStyle(.secondary) }
                    }
                }
                .foregroundStyle(.primary)
            }
        }
    }

    private var title: String {
        switch request.kind {
        case .new: t("compose_new")
        case .reply, .replyAll: t("compose_reply")
        case .forward: t("compose_forward")
        }
    }

    private func close(_ model: ComposeModel) {
        model.discardFiles()
        dismiss()
    }

    private func send(_ model: ComposeModel) {
        Task {
            do {
                try await model.send()
                dismiss()
            } catch {
                notices.error(describe(error))
            }
        }
    }
}

/// Takes a photo with the camera and hands it over as JPEG.
struct CameraPicker: UIViewControllerRepresentable {
    let done: (Data) -> Void
    @Environment(\.dismiss) private var dismiss

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.cameraCaptureMode = .photo
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: UIImagePickerController, context: Context) {}

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let parent: CameraPicker
        init(_ parent: CameraPicker) { self.parent = parent }

        func imagePickerController(_ picker: UIImagePickerController,
                                   didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]) {
            if let image = info[.originalImage] as? UIImage, let data = image.jpegData(compressionQuality: 0.85) {
                parent.done(data)
            }
            parent.dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { parent.dismiss() }
    }
}
