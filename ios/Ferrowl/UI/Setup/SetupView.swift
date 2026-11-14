import SwiftUI

/// What the setup form holds (as `SetupState` on Android).
struct SetupForm {
    var provider: Int?
    var name = ""
    var email = ""
    var password = ""
    var pop3 = false
    var username = ""
    var incomingHost = ""
    var incomingPort = "993"
    var smtpHost = ""
    var smtpPort = "465"

    func chosen(_ providers: [Provider]) -> Provider? {
        provider.flatMap { providers.indices.contains($0) ? providers[$0] : nil }
    }

    func toAccount(_ providers: [Provider]) -> Account? {
        let p = chosen(providers)
        let email = email.trimmingCharacters(in: .whitespaces)
        guard let inPort = p.map({ $0.imapPort }) ?? UInt16(incomingPort),
              let outPort = p.map({ $0.smtpPort }) ?? UInt16(smtpPort)
        else { return nil }
        let user = username.trimmingCharacters(in: .whitespaces)
        return Account(
            displayName: name.trimmingCharacters(in: .whitespaces),
            email: email,
            username: p != nil || user.isEmpty ? email : user,
            password: password,
            incomingHost: p?.imapHost ?? incomingHost.trimmingCharacters(in: .whitespaces),
            incomingPort: inPort,
            smtpHost: p?.smtpHost ?? smtpHost.trimmingCharacters(in: .whitespaces),
            smtpPort: outPort,
            protocol: p == nil && pop3 ? .pop3 : .imap,
            pop3KeepDays: nil,
            pgpPassphrase: "",
            carddavUrl: "",
            caldavUrl: "",
            oauth: nil
        )
    }
}

/// Adding an account: pick the provider (found from the address when
/// known), then the password, Sign in with Microsoft, or the servers.
struct SetupView: View {
    /// The first account (no Cancel button) or one more.
    var first = false
    var onDone: () -> Void = {}

    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.openURL) private var openURL

    private let providers = knownProviders()
    @State private var form = SetupForm()
    @State private var busy = false
    @State private var testResult: String?
    @State private var oauth = OAuthSignIn()

    private var chosen: Provider? { form.chosen(providers) }
    private var signsIn: Bool { chosen?.login == .microsoftSignIn }
    /// Gmail: Sign in with Google, or an app password.
    private var isGoogle: Bool { chosen?.imapHost == "imap.gmail.com" && oauthAvailable(provider: .google) }

    private var canConnect: Bool {
        guard !busy, !signsIn, form.email.contains("@"), !form.password.isEmpty else { return false }
        if chosen != nil { return true }
        return !form.incomingHost.trimmingCharacters(in: .whitespaces).isEmpty
            && !form.smtpHost.trimmingCharacters(in: .whitespaces).isEmpty
            && UInt16(form.incomingPort) != nil && UInt16(form.smtpPort) != nil
    }

    var body: some View {
        Form {
            if first {
                Section {
                    VStack(alignment: .leading, spacing: 8) {
                        Image("Logo").resizable().frame(width: 64, height: 64)
                        Text(t("setup_title")).font(.title2.bold())
                        Text(t("setup_desc")).foregroundStyle(.secondary)
                    }
                    .listRowBackground(Color.clear)
                }
            }

            Section {
                TextField(t("display_name"), text: $form.name)
                    .textContentType(.name)
                TextField(t("email"), text: $form.email)
                    .textContentType(.emailAddress)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .onChange(of: form.email) { _, v in
                        // Typing the address selects the provider when the domain is known.
                        if let found = providerForEmail(email: v) { form.provider = Int(found) }
                    }
                Picker(t("provider_label"), selection: $form.provider) {
                    ForEach(providers.indices, id: \.self) { i in
                        Text(providers[i].name).tag(Int?.some(i))
                    }
                    Text(t("provider_other")).tag(Int?.none)
                }
            }

            if signsIn {
                Section {
                    Button {
                        signIn(.microsoft)
                    } label: {
                        Label(t("oauth_microsoft"), systemImage: "person.badge.key")
                    }
                    .disabled(busy || !oauthAvailable(provider: .microsoft))
                } footer: {
                    Text(t("ms_signin_hint"))
                }
            } else {
                if isGoogle {
                    Section {
                        Button {
                            signIn(.google)
                        } label: {
                            Label(t("oauth_google"), systemImage: "person.badge.key")
                        }
                        .disabled(busy)
                    } footer: {
                        Text(t("oauth_or_app_password"))
                    }
                }
                Section {
                    SecureField(t("password"), text: $form.password)
                        .textContentType(.password)
                } footer: {
                    if let p = chosen, p.login == .appPassword {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(t("app_password_hint", ["provider": p.name]))
                            if let guide = p.guideUrl, let url = URL(string: guide) {
                                Button(t("app_password_howto")) { openURL(url) }
                                    .font(.footnote)
                            }
                        }
                    }
                }
            }

            if chosen == nil {
                Section {
                    Picker(t("account_type"), selection: $form.pop3) {
                        Text("IMAP").tag(false)
                        Text("POP3").tag(true)
                    }
                    .pickerStyle(.segmented)
                    .onChange(of: form.pop3) { _, on in
                        if on, form.incomingPort == "993" { form.incomingPort = "995" }
                        if !on, form.incomingPort == "995" { form.incomingPort = "993" }
                    }
                    TextField(t("username"), text: $form.username, prompt: Text(form.email.isEmpty ? t("username") : form.email))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    server(form.pop3 ? t("pop3_server") : t("imap_server"), host: $form.incomingHost, port: $form.incomingPort)
                    server(t("smtp_server"), host: $form.smtpHost, port: $form.smtpPort)
                } header: {
                    Text(t("account_details"))
                } footer: {
                    Text(form.pop3 ? t("server_hint_pop3") : t("server_hint"))
                }
            }

            if !signsIn {
                Section {
                    Button {
                        connect()
                    } label: {
                        HStack {
                            Text(t("save")).bold()
                            Spacer()
                            if busy { ProgressView() }
                        }
                    }
                    .disabled(!canConnect)
                    Button(t("test_connection")) { test() }
                        .disabled(!canConnect)
                } footer: {
                    if let testResult { Text(testResult) }
                }
            }
        }
        .navigationTitle(first ? "Ferrowl" : t("new_account"))
        .navigationBarTitleDisplayMode(first ? .large : .inline)
        .toolbar {
            if !first {
                ToolbarItem(placement: .cancellationAction) {
                    Button(t("cancel")) { onDone() }
                }
            }
        }
        .disabled(busy)
    }

    private func server(_ label: String, host: Binding<String>, port: Binding<String>) -> some View {
        HStack {
            TextField(label, text: host)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
            TextField(t("port"), text: port)
                .keyboardType(.numberPad)
                .frame(width: 64)
                .multilineTextAlignment(.trailing)
                .onChange(of: port.wrappedValue) { _, v in
                    let digits = v.filter(\.isNumber)
                    if digits != v { port.wrappedValue = digits }
                }
        }
    }

    private func connect() {
        guard let account = form.toAccount(providers) else { return }
        busy = true
        testResult = nil
        Task {
            defer { busy = false }
            do {
                try await mail.setUp(account)
                onDone()
                // Set up again (maybe a new password): the push server gets
                // the new settings, only once they work. The list of accounts
                // may be the same, so the sync is asked here.
                await app.push.forget(account.email)
                do {
                    try await app.push.sync(app)
                } catch {
                    notices.error(describePush(error))
                }
            } catch {
                notices.error(describe(error))
            }
        }
    }

    private func test() {
        guard let account = form.toAccount(providers) else { return }
        busy = true
        testResult = nil
        Task {
            defer { busy = false }
            do {
                testResult = try await mail.test(account)
            } catch {
                notices.error(describe(error))
            }
        }
    }

    private func signIn(_ provider: OAuthProvider) {
        busy = true
        Task {
            defer { busy = false }
            do {
                let email = form.email.trimmingCharacters(in: .whitespaces)
                guard let signedIn = try await oauth.signIn(provider, email: email) else { return }
                let name = form.name.trimmingCharacters(in: .whitespaces)
                try await mail.setUp(oauthAccount(provider: provider, email: signedIn, displayName: name.isEmpty ? signedIn : name))
                onDone()
            } catch {
                notices.error(describe(error))
            }
        }
    }
}
