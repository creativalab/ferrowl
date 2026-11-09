import Foundation
import Observation
import StoreKit

/// Ferrowl Pro: a yearly subscription (product `ferrowl_pro`, the same id
/// as on Google Play) that unlocks more than one mail account. The first
/// account is always free.
///
/// StoreKit 2: the App Store is asked at start-up and whenever the app
/// comes back, and transactions made elsewhere (another device, Settings)
/// arrive through `Transaction.updates`.
@MainActor
@Observable
final class ProStore {
    static let product = "ferrowl_pro"

    /// Development and TestFlight builds always have Pro (as on Android),
    /// so several accounts can be tried without a paid subscription.
    #if DEBUG
    private static let unlocked = true
    #else
    private static let unlocked = isTestFlight
    #endif

    /// TestFlight installs carry a sandbox receipt (App Store ones a
    /// regular one): known at once, before the App Store answers anything.
    private static let isTestFlight = Bundle.main.appStoreReceiptURL?.lastPathComponent == "sandboxReceipt"

    private(set) var pro = ProStore.unlocked
    /// Yearly price as the App Store shows it, once known.
    private(set) var price: String?
    private var item: Product?
    private var updates: Task<Void, Never>?

    func start() {
        if updates == nil {
            updates = Task { [weak self] in
                for await result in Transaction.updates {
                    if case .verified(let transaction) = result { await transaction.finish() }
                    await self?.refresh()
                }
            }
        }
        Task { await refresh() }
    }

    func refresh() async {
        if item == nil, let found = try? await Product.products(for: [Self.product]).first {
            item = found
            price = found.displayPrice
        }
        var active = false
        for await result in Transaction.currentEntitlements {
            if case .verified(let t) = result, t.productID == Self.product, t.revocationDate == nil {
                active = true
            }
        }
        pro = Self.unlocked || active
    }

    enum Outcome { case bought, cancelled, unavailable }

    func buy() async -> Outcome {
        if item == nil { await refresh() }
        guard let item else { return .unavailable }
        do {
            switch try await item.purchase() {
            case .success(let result):
                if case .verified(let transaction) = result { await transaction.finish() }
                await refresh()
                return pro ? .bought : .cancelled
            case .pending, .userCancelled:
                return .cancelled
            @unknown default:
                return .cancelled
            }
        } catch {
            return .unavailable
        }
    }

    /// "Restore purchases": asks the App Store again for this Apple ID.
    func restore() async {
        try? await AppStore.sync()
        await refresh()
    }
}
