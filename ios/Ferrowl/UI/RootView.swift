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
            NoticeCard(notice: notice, act: notices.act, dismiss: notices.dismiss)
                .padding(.horizontal, 12)
                .padding(.bottom, Self.clearance)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                .id(notice.id)
                .animation(.snappy, value: notice.id)
        }
    }
}

/// One notice: dark in both themes (as the system's own banners), so it
/// stands out over the list; an Undo shows how long it is still offered.
private struct NoticeCard: View {
    let notice: Notices.Notice
    let act: () -> Void
    let dismiss: () -> Void

    /// What is left of the time to undo (1 → 0).
    @State private var left: CGFloat = 1

    private static let background = Color(white: 0.14)
    private static let seconds = Double(Notices.duration.components.seconds)

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: notice.isError ? "exclamationmark" : "checkmark")
                .font(.system(size: 15, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 34, height: 34)
                .background(Circle().fill(notice.isError ? Color.red : Color.accentColor))
            Text(notice.text)
                .font(.body.weight(.medium))
                .foregroundStyle(.white)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let label = notice.actionLabel {
                Button(action: act) {
                    Text(label)
                        .font(.subheadline.bold())
                        .foregroundStyle(.white)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 9)
                        .background(Capsule().fill(Color.accentColor))
                }
                .buttonStyle(.plain)
            }
            if notice.isError {
                Button(action: dismiss) {
                    Image(systemName: "xmark")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.7))
                        .frame(width: 32, height: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(t("close"))
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, notice.actionLabel == nil && !notice.isError ? 18 : 12)
        .padding(.vertical, 14)
        .frame(minHeight: 64)
        .background(Self.background)
        // The time left to undo, as a bar shrinking along the bottom.
        .overlay(alignment: .bottomLeading) {
            if notice.actionLabel != nil {
                GeometryReader { geo in
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(width: geo.size.width * left, height: 3)
                        .frame(maxHeight: .infinity, alignment: .bottom)
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))
        // An edge that separates it from a dark screen too.
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.white.opacity(0.1)))
        .shadow(color: .black.opacity(0.28), radius: 14, y: 6)
        .contentShape(Rectangle())
        .onTapGesture { if notice.actionLabel == nil { dismiss() } }
        .accessibilityElement(children: .combine)
        .onAppear {
            guard notice.actionLabel != nil else { return }
            withAnimation(.linear(duration: Self.seconds)) { left = 0 }
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
