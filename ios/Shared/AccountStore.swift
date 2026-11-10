import Foundation

/// Saves the accounts on the phone: settings in a JSON file in the app's
/// private storage (left out of backups), passwords and OpenPGP
/// passphrases in the Keychain.
///
/// Several accounts, one of them active (the one shown and connected),
/// identified by their address, in the order they were added.
struct AccountStore {
    private static let secrets = "com.ferrowl.app.accounts"

    private let file: URL

    init(directory: URL) {
        file = directory.appendingPathComponent("accounts.json")
    }

    struct Snapshot {
        var accounts: [Account]
        var active: String?
    }

    func load() -> Snapshot {
        guard let data = try? Data(contentsOf: file),
              let saved = try? JSONDecoder().decode(Saved.self, from: data)
        else { return Snapshot(accounts: [], active: nil) }
        let accounts = saved.accounts.map { $0.account() }
        let active = accounts.first { $0.email.caseInsensitiveCompare(saved.active ?? "") == .orderedSame }
            ?? accounts.first
        return Snapshot(accounts: accounts, active: active?.email)
    }

    func save(_ snapshot: Snapshot) {
        for a in snapshot.accounts {
            Self.storeSecret("password", a.email, a.password)
            Self.storeSecret("pgp", a.email, a.pgpPassphrase)
            Self.storeSecret("dav", a.email, a.davPassword)
        }
        let saved = Saved(accounts: snapshot.accounts.map(Stored.init), active: snapshot.active)
        guard let data = try? JSONEncoder().encode(saved) else { return }
        try? data.write(to: file, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
        var url = file
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? url.setResourceValues(values)
    }

    /// Forgets the secrets of a removed account.
    func removeSecrets(of email: String) {
        Keychain.delete(Self.secrets, "password:\(email.lowercased())")
        Keychain.delete(Self.secrets, "pgp:\(email.lowercased())")
        Keychain.delete(Self.secrets, "dav:\(email.lowercased())")
    }

    private static func storeSecret(_ kind: String, _ email: String, _ value: String) {
        let key = "\(kind):\(email.lowercased())"
        if value.isEmpty {
            Keychain.delete(secrets, key)
        } else {
            Keychain.set(secrets, key, value)
        }
    }

    private static func secret(_ kind: String, _ email: String) -> String {
        Keychain.get(secrets, "\(kind):\(email.lowercased())") ?? ""
    }

    private struct Saved: Codable {
        var accounts: [Stored]
        var active: String?
    }

    /// What is written to the file: the engine's Account without secrets.
    private struct Stored: Codable {
        var displayName: String
        var email: String
        var username: String
        var incomingHost: String
        var incomingPort: UInt16
        var smtpHost: String
        var smtpPort: UInt16
        var pop3: Bool
        var pop3KeepDays: UInt32?
        var carddavUrl: String
        var caldavUrl: String
        /// Contacts/calendar servers' own username (its password: Keychain).
        var davUsername: String?
        /// "microsoft" / "google" when the account signs in instead of a password.
        var oauth: String?

        init(_ a: Account) {
            displayName = a.displayName
            email = a.email
            username = a.username
            incomingHost = a.incomingHost
            incomingPort = a.incomingPort
            smtpHost = a.smtpHost
            smtpPort = a.smtpPort
            pop3 = a.protocol == .pop3
            pop3KeepDays = a.pop3KeepDays
            carddavUrl = a.carddavUrl
            caldavUrl = a.caldavUrl
            davUsername = a.davUsername.isEmpty ? nil : a.davUsername
            oauth = switch a.oauth {
            case .microsoft: "microsoft"
            case .google: "google"
            case nil: nil
            }
        }

        func account() -> Account {
            let provider: OAuthProvider? = switch oauth {
            case "microsoft": .microsoft
            case "google": .google
            default: nil
            }
            return Account(
                displayName: displayName,
                email: email,
                username: username,
                password: AccountStore.secret("password", email),
                incomingHost: incomingHost,
                incomingPort: incomingPort,
                smtpHost: smtpHost,
                smtpPort: smtpPort,
                protocol: pop3 ? .pop3 : .imap,
                pop3KeepDays: pop3KeepDays,
                pgpPassphrase: AccountStore.secret("pgp", email),
                carddavUrl: carddavUrl,
                caldavUrl: caldavUrl,
                davUsername: davUsername ?? "",
                davPassword: AccountStore.secret("dav", email),
                oauth: provider
            )
        }
    }
}
