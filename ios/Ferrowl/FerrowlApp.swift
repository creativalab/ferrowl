import CoreSpotlight
import SwiftUI
import UserNotifications

/// Everything shared by the screens: one per process.
@MainActor
@Observable
final class AppModel {
    let engine: Engine
    let notices = Notices()
    let prefs = Preferences()
    let pro = ProStore()
    /// Made after the shared settings are in place (it reads its
    /// registration from there).
    let push: PushRegistrar
    let mail: MailRepository
    /// A second engine on the same files, to check the accounts that are
    /// not the active one without disconnecting it.
    let side: Engine
    private let banners = ForegroundBanners()
    /// A notification was tapped: the message to open (account, uid; no
    /// uid = its inbox).
    var pendingOpen: (email: String, uid: UInt32?)?
    /// A `ferrowl://compose`/`mailto:` link or a shortcut asked for a new
    /// message: shown to the user, never sent by itself.
    var pendingCompose: ComposeRequest?

    /// The process's model, for Shortcuts/Siri actions (they run in the
    /// app's process, outside the views).
    private(set) static var shared: AppModel?

    init() {
        // Shared with the notification extension (see SharedStorage).
        SharedStorage.migrate()
        // Copies of attached files left by composers of an earlier run.
        try? FileManager.default.removeItem(at: FileManager.default.temporaryDirectory.appendingPathComponent("outgoing"))
        push = PushRegistrar()
        let support = SharedStorage.directory
        let data = SharedStorage.mail

        // Sign-in tokens go to the Keychain.
        setTokenVault(vault: KeychainTokenVault())
        engine = Engine(dataDir: data.path, language: Language.code)
        side = Engine(dataDir: data.path, language: Language.code)
        UNUserNotificationCenter.current().delegate = banners
        mail = MailRepository(engine: engine, notices: notices, directory: support)
        banners.open = { [weak self] email, uid in self?.pendingOpen = (email, uid) }
        banners.isActive = { [weak self] email in
            email.caseInsensitiveCompare(self?.mail.activeEmail ?? "") == .orderedSame
        }
        banners.arrived = { [weak self] email in self?.mail.arrived(email) }
        Self.shared = self
        // The widget's "read" and "archive" buttons (they run here).
        WidgetActions.run = { [weak self] link, archive in
            guard let self, case .open(let email, let folder, let uid) = parseLink(url: link),
                  let account = self.mail.accounts.first(where: { $0.email.caseInsensitiveCompare(email) == .orderedSame })
            else { return }
            // It leaves the widget at once, and counts one less on the icon
            // and in the sidebar too; all put back if the server says no.
            let widget = WidgetData.load()
            let count = Badge.count(for: account.email)
            let listed = widget.contains { $0.recent.contains { $0.link == link } }
            WidgetData.removeMail(link: link)
            if listed { Badge.set(max(0, count - 1), for: account.email) }
            Badge.show()
            do {
                // An engine of its own: the app's may be busy, or on another account.
                let engine = Engine(dataDir: SharedStorage.mail.path, language: Language.code)
                _ = try await engine.connect(account: account)
                if archive {
                    _ = try await engine.archiveMany(folderId: folder, uids: [uid])
                } else {
                    try await engine.setSeenMany(folderId: folder, uids: [uid], seen: true)
                }
            } catch {
                if listed { Badge.set(count, for: account.email) }
                WidgetData.restore(widget)
                Badge.show()
                throw error
            }
            self.mail.touch()
        }
    }

    /// A `ferrowl://` or `mailto:` link opened from another app, or a file
    /// shared to Ferrowl (it becomes a new message with it attached).
    func open(link url: URL) {
        if url.isFileURL {
            pendingCompose = ComposeRequest(files: [url])
            return
        }
        switch parseLink(url: url.absoluteString) {
        case .open(let account, _, let uid):
            // Only inbox messages can be opened directly for now.
            pendingOpen = (account, uid)
        case .inbox(let account):
            if let email = account ?? mail.activeEmail { pendingOpen = (email, nil) }
        case .compose(let to, let cc, _, let subject, let body):
            pendingCompose = ComposeRequest(to: to, cc: cc, subject: subject, body: body)
        case nil:
            break
        }
    }

    /// Whether an account may be used: the first is free, the others need
    /// Ferrowl Pro.
    func mayUse(_ email: String) -> Bool {
        pro.pro || mail.accounts.first?.email.caseInsensitiveCompare(email) == .orderedSame
    }

    /// Whether another account may be added (Ferrowl Pro after the first).
    var mayAddAccount: Bool { pro.pro || mail.accounts.isEmpty }
}

/// New-mail notifications also show while the app is open.
/// New-mail notifications also show while the app is open; tapping one
/// opens its message.
final class ForegroundBanners: NSObject, UNUserNotificationCenterDelegate {
    var open: ((String, UInt32?) -> Void)?
    /// Whether an address is the account on screen.
    var isActive: ((String) -> Bool)?
    /// New mail for an account while the app is open.
    var arrived: ((String) -> Void)?

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
                                withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        let info = notification.request.content.userInfo
        let email = info["email"] as? String
            ?? MailNotifications.account(pushId: info["account"] as? String)?.email
        guard let email else { return done([.banner, .list, .sound]) }
        DispatchQueue.main.async {
            // The account on screen: its list shows the new message at once,
            // so no banner over it (a sound, and it stays in the list of
            // notifications). Other accounts: a banner as usual.
            let onScreen = self.isActive?(email) ?? false
            self.arrived?(email)
            done(onScreen ? [.list, .sound] : [.banner, .list, .sound])
        }
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
                                withCompletionHandler done: @escaping () -> Void) {
        let info = response.notification.request.content.userInfo
        if let email = info["email"] as? String {
            let uid = (info["uid"] as? NSNumber)?.uint32Value ?? (info["uid"] as? Int).map { UInt32($0) }
            DispatchQueue.main.async { self.open?(email, uid) }
        }
        done()
    }
}

@main
struct FerrowlApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var app: AppModel
    @Environment(\.scenePhase) private var phase

    init() {
        let app = AppModel()
        _app = State(initialValue: app)
        BackgroundRefresh.register(app)
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .onOpenURL { app.open(link: $0) }
                // A message found in the iPhone's search.
                .onContinueUserActivity(CSSearchableItemActionType) { activity in
                    if let id = activity.userInfo?[CSSearchableItemActivityIdentifier] as? String, let url = URL(string: id) {
                        app.open(link: url)
                    }
                }
                .onAppear {
                    delegate.push = app.push
                    // WebKit starts now rather than at the first message opened.
                    HTMLView.prewarm()
                }
                .environment(app)
                .environment(app.mail)
                .environment(app.notices)
                .environment(app.prefs)
                .environment(app.pro)
                .tint(Theme.current(app.prefs.theme).accent)
                .preferredColorScheme(Theme.current(app.prefs.theme).scheme)
        }
        .onChange(of: phase) { _, now in
            switch now {
            case .active:
                // Ferrowl Pro may have been bought or cancelled elsewhere meanwhile.
                app.pro.start()
            case .background:
                if app.prefs.notifications, app.mail.activeEmail != nil { BackgroundRefresh.schedule() }
                // Deletes waiting for Undo are done now: iOS may stop the app
                // before their notice is gone. A little time is asked for them.
                let mail = app.mail
                let task = UIApplication.shared.beginBackgroundTask()
                Task {
                    await mail.flushDeferred()
                    UIApplication.shared.endBackgroundTask(task)
                }
            default:
                break
            }
        }
    }
}
