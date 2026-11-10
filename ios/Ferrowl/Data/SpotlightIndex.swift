import CoreSpotlight
import UniformTypeIdentifiers

/// The inbox's newest messages in the iPhone's search (Spotlight), on the
/// phone only: found by sender or subject, opened in Ferrowl with a tap
/// (their identifier is the message's `ferrowl://open` link).
enum SpotlightIndex {
    /// Messages kept in the index per account.
    private static let limit = 300

    /// Replaces an account's messages in the index with these.
    static func index(_ headers: [Header], account: String) {
        let domain = account.lowercased()
        let items = headers.prefix(limit).map { h in
            let attributes = CSSearchableItemAttributeSet(contentType: .emailMessage)
            attributes.title = h.subject.isEmpty ? t("no_subject") : h.subject
            attributes.contentDescription = h.preview
            attributes.authorNames = [h.from.isEmpty ? h.fromAddress : h.from]
            attributes.authorEmailAddresses = [h.fromAddress]
            attributes.contentCreationDate = Date(timeIntervalSince1970: TimeInterval(h.timestamp))
            return CSSearchableItem(
                uniqueIdentifier: messageLink(account: account, folder: "INBOX", uid: h.uid),
                domainIdentifier: domain,
                attributeSet: attributes
            )
        }
        let index = CSSearchableIndex.default()
        index.deleteSearchableItems(withDomainIdentifiers: [domain]) { _ in
            index.indexSearchableItems(Array(items)) { _ in }
        }
    }

    /// An account removed (or the setting turned off): its messages leave.
    static func remove(account: String) {
        CSSearchableIndex.default().deleteSearchableItems(withDomainIdentifiers: [account.lowercased()]) { _ in }
    }

    static func removeAll() {
        CSSearchableIndex.default().deleteAllSearchableItems { _ in }
    }
}
