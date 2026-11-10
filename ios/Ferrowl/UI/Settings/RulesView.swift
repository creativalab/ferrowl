import SwiftUI

/// Settings → Rules: "when a message arrives that … then …" for the account
/// in use (Ferrowl Pro). A condition is a search; the preview shows which
/// messages on the phone it picks.
struct RulesView: View {
    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @State private var rules: [Rule] = []
    @State private var editing: Rule?

    var body: some View {
        Form {
            Section {
                Text(t("rules_desc")).font(.footnote).foregroundStyle(.secondary)
                if !app.pro.pro {
                    Label(t("rules_pro"), systemImage: "lock").font(.footnote)
                }
            }
            Section {
                if rules.isEmpty {
                    Text(t("rules_none")).foregroundStyle(.secondary)
                }
                ForEach(rules, id: \.id) { rule in
                    Button { editing = rule } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(rule.name.isEmpty ? rule.condition : rule.name)
                                .foregroundStyle(rule.enabled ? .primary : .secondary)
                            Text(RuleEditor.summary(rule.actions, folders: mail.folders))
                                .font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .onDelete { offsets in
                    for i in offsets {
                        try? mail.deleteRule(rules[i].id)
                    }
                    reload()
                }
                Button {
                    editing = Rule(id: UUID().uuidString, name: "", condition: "", actions: [.markRead], enabled: true)
                } label: {
                    Label(t("rules_add"), systemImage: "plus")
                }
                .disabled(!app.pro.pro)
            }
        }
        .navigationTitle(t("rules_title"))
        .onAppear(perform: reload)
        .sheet(item: Binding(get: { editing.map(EditingRule.init) }, set: { editing = $0?.rule })) { item in
            NavigationStack {
                RuleEditor(rule: item.rule) { saved in
                    do {
                        try mail.saveRule(saved)
                        notices.info(t("rules_saved"))
                    } catch {
                        notices.error(describe(error))
                    }
                    editing = nil
                    reload()
                }
            }
            .showsNotices()
        }
    }

    private func reload() { rules = mail.rules() }

    private struct EditingRule: Identifiable {
        let rule: Rule
        var id: String { rule.id }
    }
}

/// Writing a rule: its condition (with a preview), what it does, its name.
struct RuleEditor: View {
    @State var rule: Rule
    let save: (Rule) -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(\.dismiss) private var dismiss
    @State private var preview: [Header] = []
    @State private var picking = false

    /// Where matching mail goes: nowhere, the Archive or a folder.
    private var destination: RuleAction? {
        rule.actions.first { if case .archive = $0 { true } else if case .moveTo = $0 { true } else { false } }
    }

    var body: some View {
        Form {
            Section {
                TextField(t("rules_condition_hint"), text: $rule.condition)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            } header: {
                Text(t("rules_condition"))
            } footer: {
                Text(t("rules_condition_help"))
            }
            Section(t("rules_preview")) {
                if preview.isEmpty {
                    Text(t("rules_preview_none")).foregroundStyle(.secondary)
                }
                ForEach(preview, id: \.uid) { h in
                    VStack(alignment: .leading) {
                        Text(h.from.isEmpty ? h.fromAddress : h.from).font(.subheadline)
                        Text(h.subject).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
            }
            Section(t("rules_then")) {
                toggle(t("rules_mark_read"), .markRead)
                toggle(t("rules_star"), .star)
                toggle(t("rules_silence"), .silence)
                Picker(t("rules_destination"), selection: Binding(
                    get: { destinationTag },
                    set: { tag in
                        if tag == "pick" { picking = true } else { setDestination(tag == "archive" ? .archive : nil) }
                    }
                )) {
                    Text(t("rules_stay")).tag("none")
                    Text(t("archive")).tag("archive")
                    if case .moveTo(let folder) = destination {
                        Text(mail.folder(folder)?.name ?? folder).tag("folder")
                    }
                    Text(t("rules_pick_folder")).tag("pick")
                }
            }
            Section {
                TextField(t("rules_name"), text: $rule.name)
                Toggle(t("rules_enabled"), isOn: $rule.enabled)
            }
        }
        .navigationTitle(t("rules_title"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(t("cancel")) { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button(t("rules_save")) { save(rule) }
                    .disabled(rule.condition.trimmingCharacters(in: .whitespaces).isEmpty || rule.actions.isEmpty)
            }
        }
        .task(id: rule.condition) {
            try? await Task.sleep(for: .milliseconds(300))
            preview = await mail.rulePreview(rule.condition)
        }
        .sheet(isPresented: $picking) {
            FolderPicker(exclude: mail.inboxId) { folder in setDestination(.moveTo(folder: folder)) }
        }
    }

    private var destinationTag: String {
        switch destination {
        case .archive: "archive"
        case .moveTo: "folder"
        default: "none"
        }
    }

    private func setDestination(_ action: RuleAction?) {
        rule.actions.removeAll { if case .archive = $0 { true } else if case .moveTo = $0 { true } else { false } }
        if let action { rule.actions.append(action) }
    }

    private func toggle(_ title: String, _ action: RuleAction) -> some View {
        Toggle(title, isOn: Binding(
            get: { rule.actions.contains(action) },
            set: { on in
                rule.actions.removeAll { $0 == action }
                if on { rule.actions.append(action) }
            }
        ))
    }

    /// "Mark read · Archive" for the list of rules.
    static func summary(_ actions: [RuleAction], folders: [Folder]) -> String {
        actions.map { action in
            switch action {
            case .markRead: t("rules_mark_read")
            case .star: t("rules_star")
            case .silence: t("rules_silence")
            case .archive: t("archive")
            case .moveTo(let folder): "→ \(folders.first { $0.id == folder }?.name ?? folder)"
            }
        }.joined(separator: " · ")
    }
}
