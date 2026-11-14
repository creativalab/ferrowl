import SwiftUI

/// Account setup the first time, the mailbox after; notices on top of both.
struct RootView: View {
    @Environment(AppModel.self) private var app
    @Environment(MailRepository.self) private var mail
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        Group {
            if mail.accounts.isEmpty {
                NavigationStack { SetupView(first: true) }
            } else {
                MainView()
                    // A new active account starts the mailbox over.
                    .id(mail.activeEmail ?? "")
            }
        }
        // Switching account: connecting can take a few seconds.
        .overlay {
            if let email = mail.switching {
                ZStack {
                    Color.black.opacity(0.25).ignoresSafeArea()
                    VStack(spacing: 12) {
                        ProgressView().controlSize(.large)
                        Text(t("status_connecting")).font(.headline)
                        Text(email).font(.subheadline).foregroundStyle(.secondary)
                    }
                    .padding(28)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18))
                }
                .transition(.opacity)
                .accessibilityElement(children: .combine)
            }
        }
        .animation(.easeInOut(duration: 0.2), value: mail.switching)
        .overlay(alignment: .bottom) { NoticeBanner() }
        // Instant notifications: what they send to the server is explained
        // once, with the first account, before anything is sent.
        .sheet(isPresented: Binding(get: { !mail.accounts.isEmpty && !app.push.asked }, set: { _ in })) {
            PushConsent()
                .interactiveDismissDisabled()
                .presentationDetents([.medium, .large])
        }
        .task { app.pro.start() }
        // While the app is open: new mail of every account every few minutes.
        .task(id: scenePhase == .active && !mail.accounts.isEmpty) {
            if scenePhase == .active, !mail.accounts.isEmpty { await BackgroundRefresh.watch(app) }
        }
        // Instant notifications: the push server follows the accounts (and
        // gets the key again after it restarted).
        .task(id: "\(app.pro.pro)|\(app.pro.known)|\(mail.accounts.map(\.email).joined(separator: ","))") {
            do {
                try await app.push.sync(app)
            } catch {
                app.notices.error(describePush(error))
            }
        }
        // Notifications are asked for once there is mail to announce.
        .task(id: mail.accounts.isEmpty) {
            if !mail.accounts.isEmpty { BackgroundRefresh.askPermission() }
        }
        // Without Ferrowl Pro only the first account may stay active (also
        // when the subscription ends while another one is in use).
        .task(id: "\(app.pro.pro)|\(app.pro.known)|\(mail.activeEmail ?? "")|\(mail.accounts.count)") {
            // Only once the subscriptions were read (the remembered state
            // until then), and not while a message is being written.
            guard app.pro.known, !app.pro.pro, let active = mail.activeEmail, !app.mayUse(active) else { return }
            await mail.afterCompose()
            guard !Task.isCancelled, !app.pro.pro else { return }
            await mail.backToFirst()
        }
    }
}

/// The notice at the bottom of the screen: a confirmation or an error,
/// maybe with a button (Undo). At the top it covered the navigation bar's
/// buttons; at the bottom it stays above the search field, the compose
/// button and a message's actions, and Undo is within the thumb's reach.
struct NoticeBanner: View {
    @Environment(Notices.self) private var notices

    /// Room left below: the bottom search field (about 60 points) with the
    /// compose button above it (20 + 48), so the notice never covers it.
    static let clearance: CGFloat = 132

    var body: some View {
        if let notice = notices.current {
            HStack(spacing: 12) {
                Image(systemName: notice.isError ? "exclamationmark.triangle.fill" : "checkmark.circle.fill")
                    .foregroundStyle(notice.isError ? .red : .accentColor)
                Text(notice.text)
                    .font(.subheadline)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let label = notice.actionLabel {
                    Button(label) { notices.act() }
                        .font(.subheadline.bold())
                }
                if notice.isError {
                    Button { notices.dismiss() } label: {
                        Image(systemName: "xmark").font(.subheadline.weight(.semibold))
                    }
                    .foregroundStyle(.secondary)
                    .accessibilityLabel(t("close"))
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14))
            .shadow(color: .black.opacity(0.12), radius: 8, y: 2)
            .padding(.horizontal, 12)
            .padding(.bottom, Self.clearance)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .onTapGesture { if notice.actionLabel == nil { notices.dismiss() } }
            .accessibilityAddTraits(.isStaticText)
            .id(notice.id)
            .animation(.snappy, value: notice.id)
        }
    }
}

extension View {
    /// Confirmations and errors on top of a sheet too (a sheet covers the
    /// app's own notice).
    func showsNotices() -> some View {
        overlay(alignment: .bottom) { NoticeBanner() }
    }
}

/// Asked once: instant notifications through push.ferrowl.com, or not.
struct PushConsent: View {
    @Environment(AppModel.self) private var app
    @Environment(Notices.self) private var notices
    @State private var busy = false

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                Image(systemName: "bell.badge")
                    .font(.system(size: 44))
                    .foregroundStyle(.tint)
                    .padding(.top, 24)
                Text(t("push_title")).font(.title2.bold())
                Text(t("push_desc"))
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button {
                    busy = true
                    Task {
                        defer { busy = false }
                        do {
                            try await app.push.enable(app)
                        } catch {
                            notices.error(describePush(error))
                            await app.push.disable()
                        }
                    }
                } label: {
                    HStack {
                        if busy { ProgressView() }
                        Text(t("push_enable")).bold()
                    }
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(busy)
                Button(t("pro_not_now")) { Task { await app.push.disable() } }
                    .disabled(busy)
            }
            .padding(24)
        }
    }
}
