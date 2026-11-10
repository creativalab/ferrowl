import StoreKit
import SwiftUI

struct SettingsView: View {
    let addAccount: () -> Void

    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Preferences.self) private var prefs
    @Environment(ProStore.self) private var pro
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss
    @State private var offer = false
    @State private var manage = false
    @State private var pushBusy = false

    var body: some View {
        @Bindable var prefs = prefs
        NavigationStack {
            Form {
                Section(t("section_accounts")) {
                    ForEach(mail.accounts, id: \.email) { account in
                        NavigationLink {
                            AccountDetail(account: account, closeSettings: { dismiss() })
                        } label: {
                            HStack {
                                Avatar(name: account.displayName.isEmpty ? account.email : account.displayName, size: 32)
                                VStack(alignment: .leading) {
                                    Text(account.displayName.isEmpty ? account.email : account.displayName)
                                    Text(account.email).font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                if account.email == mail.activeEmail {
                                    Image(systemName: "checkmark").foregroundStyle(.tint)
                                } else if !app.mayUse(account.email) {
                                    Text(t("account_inactive")).font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                    Button {
                        dismiss()
                        addAccount()
                    } label: {
                        Label(t("add_account"), systemImage: "plus")
                    }
                }

                Section {
                    if pro.pro {
                        Label(t("pro_thanks"), systemImage: "checkmark.seal.fill")
                        Button(t("pro_manage_ios")) { manage = true }
                    } else {
                        Button(t("pro_title")) { offer = true }
                    }
                    Button(t("pro_restore")) { Task { await pro.restore() } }
                } header: {
                    Text(t("pro_title"))
                }

                Section {
                    Picker(t("list_view"), selection: $prefs.threaded) {
                        Text(t("view_threads")).tag(true)
                        Text(t("view_messages")).tag(false)
                    }
                    Toggle(isOn: $prefs.remoteImages) {
                        VStack(alignment: .leading) {
                            Text(t("remote_images"))
                            Text(t("remote_images_desc")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Toggle(isOn: $prefs.notifications) {
                        VStack(alignment: .leading) {
                            Text(t("notif_bg"))
                            Text(t("notif_bg_desc")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .onChange(of: prefs.notifications) { _, on in
                        if on {
                            BackgroundRefresh.askPermission()
                            BackgroundRefresh.schedule()
                        }
                    }
                    Toggle(isOn: Binding(get: { app.push.enabled }, set: { on in togglePush(on) })) {
                        VStack(alignment: .leading) {
                            Text(t("push_title"))
                            Text(t("push_desc")).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    .disabled(pushBusy)
                    NavigationLink(t("senders_title")) { SenderListsView() }
                    NavigationLink(t("rules_title")) { RulesView() }
                } header: {
                    Text(t("section_general"))
                }

                Section(t("color_theme")) {
                    Picker(t("color_theme"), selection: $prefs.theme) {
                        ForEach(Theme.all) { theme in
                            HStack {
                                ThemeSwatch(theme: theme)
                                Text(t(theme.nameKey))
                            }
                            .tag(theme.id)
                        }
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                }

                Section {
                    LabeledContent("Ferrowl", value: Self.version)
                    Link("support@ferrowl.com", destination: URL(string: "mailto:support@ferrowl.com")!)
                }
            }
            .navigationTitle(t("settings"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button(t("close")) { dismiss() } }
            }
            .sheet(isPresented: $offer) { ProOffer(onBought: { offer = false }).showsNotices() }
            .manageSubscriptionsSheet(isPresented: $manage)
        }
    }

    private func togglePush(_ on: Bool) {
        pushBusy = true
        Task {
            defer { pushBusy = false }
            if on {
                do {
                    try await app.push.enable(app)
                } catch {
                    notices.error(describePush(error))
                }
            } else {
                await app.push.disable()
            }
        }
    }

    static var version: String {
        let info = Bundle.main.infoDictionary
        let v = info?["CFBundleShortVersionString"] as? String ?? ""
        let b = info?["CFBundleVersion"] as? String ?? ""
        return "\(v) (\(b))"
    }
}

/// An account's settings: the name, the contacts and calendar servers, and
/// removing it.
struct AccountDetail: View {
    let account: Account
    /// The last account is gone: Settings closes (the setup comes next).
    let closeSettings: () -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(\.dismiss) private var dismiss
    @State private var confirmRemove = false
    @State private var removing = false
    @State private var name = ""
    @State private var carddav = ""
    @State private var caldav = ""
    @State private var davUser = ""
    @State private var davPassword = ""

    private func remove() {
        removing = true
        Task {
            let left = await mail.removeAccount(account.email)
            removing = false
            if left { dismiss() } else { closeSettings() }
        }
    }

    var body: some View {
        Form {
            Section {
                TextField(t("display_name"), text: $name)
                LabeledContent(t("email"), value: account.email)
                if let provider = account.oauth {
                    LabeledContent(t("oauth_signed_in", ["provider": provider == .microsoft ? "Microsoft" : "Google"]), value: "")
                } else {
                    LabeledContent(t(account.protocol == .pop3 ? "pop3_server" : "imap_server"),
                                   value: "\(account.incomingHost):\(account.incomingPort)")
                    LabeledContent(t("smtp_server"), value: "\(account.smtpHost):\(account.smtpPort)")
                }
            }
            Section {
                TextField(t("carddav_url"), text: $carddav, prompt: Text("https://"))
                    .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                TextField(t("caldav_url"), text: $caldav, prompt: Text("https://"))
                    .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
            } footer: {
                Text(t("carddav_url_hint"))
            }
            Section {
                TextField(t("dav_username"), text: $davUser)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().textContentType(.username)
                SecureField(t("dav_password"), text: $davPassword)
            } header: {
                Text(t("dav_credentials"))
            } footer: {
                Text(t("dav_credentials_hint"))
            }
            Section {
                Button(role: .destructive) {
                    confirmRemove = true
                } label: {
                    HStack {
                        Text(t("remove_account"))
                        Spacer()
                        if removing { ProgressView() }
                    }
                }
                .disabled(removing)
            }
        }
        .navigationTitle(t("account_settings"))
        // On this screen (not the list under it), so iOS shows it.
        .confirmationDialog(t("remove_account"), isPresented: $confirmRemove, titleVisibility: .visible) {
            Button(t("remove_account"), role: .destructive) { remove() }
        } message: {
            Text(t("confirm_remove_account"))
        }
        .onAppear {
            name = account.displayName
            carddav = account.carddavUrl
            caldav = account.caldavUrl
            davUser = account.davUsername
            davPassword = account.davPassword
        }
        .onDisappear {
            guard !removing else { return }
            var changed = account
            changed.displayName = name.trimmingCharacters(in: .whitespaces)
            changed.carddavUrl = carddav.trimmingCharacters(in: .whitespaces)
            changed.caldavUrl = caldav.trimmingCharacters(in: .whitespaces)
            changed.davUsername = davUser.trimmingCharacters(in: .whitespaces)
            changed.davPassword = changed.davUsername.isEmpty ? "" : davPassword
            if changed != account { mail.update(changed) }
        }
    }
}

/// The Ferrowl Pro offer: more than one account, yearly subscription.
struct ProOffer: View {
    let onBought: () -> Void

    @Environment(ProStore.self) private var pro
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false

    var body: some View {
        NavigationStack {
            ScrollView {
            VStack(spacing: 20) {
                Image("Logo").resizable().frame(width: 72, height: 72)
                Text(t("pro_title")).font(.title.bold())
                Text(t("pro_text_ios", ["price": pro.price ?? "…"]))
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    // Wraps instead of ending in "…".
                    .fixedSize(horizontal: false, vertical: true)
                Button {
                    buy()
                } label: {
                    HStack {
                        if busy { ProgressView() }
                        Text(t("pro_subscribe")).bold()
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(busy)
                Button(t("pro_restore")) {
                    Task {
                        await pro.restore()
                        if pro.pro { onBought() }
                    }
                }
                .font(.footnote)
            }
            .padding(24)
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(t("pro_not_now")) { dismiss() } }
            }
        }
        .presentationDetents([.large])
        .task { await pro.refresh() }
    }

    private func buy() {
        busy = true
        Task {
            defer { busy = false }
            switch await pro.buy() {
            case .bought:
                notices.info(t("pro_thanks"))
                onBought()
            case .cancelled:
                break
            case .unavailable:
                notices.error(t("pro_unavailable"))
            }
        }
    }
}

/// Blocked and allowed senders (addresses or @domains).
struct SenderListsView: View {
    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @State private var blocked: [String] = []
    @State private var allowed: [String] = []
    @State private var newBlocked = ""
    @State private var newAllowed = ""

    var body: some View {
        Form {
            Section { Text(t("senders_desc")).font(.footnote).foregroundStyle(.secondary) }
            list(t("senders_blocked"), items: blocked, entry: $newBlocked, allow: false)
            list(t("senders_allowed"), items: allowed, entry: $newAllowed, allow: true)
        }
        .navigationTitle(t("senders_title"))
        .onAppear(perform: reload)
    }

    private func list(_ title: String, items: [String], entry: Binding<String>, allow: Bool) -> some View {
        Section(title) {
            if items.isEmpty { Text(t("senders_none")).foregroundStyle(.secondary) }
            ForEach(items, id: \.self) { item in
                Text(item)
                    .swipeActions {
                        Button(t("senders_remove"), role: .destructive) {
                            mail.removeSenderRule(item)
                            reload()
                        }
                    }
            }
            HStack {
                TextField(t("senders_placeholder"), text: entry)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.emailAddress)
                Button(t("senders_add")) {
                    do {
                        try mail.setSenderRule(entry.wrappedValue.trimmingCharacters(in: .whitespaces), allow: allow)
                        entry.wrappedValue = ""
                        reload()
                    } catch {
                        notices.error(describe(error))
                    }
                }
                .disabled(entry.wrappedValue.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
    }

    private func reload() {
        blocked = mail.senderRules(allow: false)
        allowed = mail.senderRules(allow: true)
    }
}
