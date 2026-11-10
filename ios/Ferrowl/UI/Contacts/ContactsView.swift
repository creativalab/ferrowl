import SwiftUI

/// Contacts from the account's CardDAV server (a copy on the phone), and
/// those written here (saved on the server, or on the phone when the
/// account has none).
struct ContactsView: View {
    let write: (String) -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var contacts: [Contact] = []
    @State private var syncing = false
    @State private var editing: Contact?

    var body: some View {
        NavigationStack {
            List(contacts, id: \.self) { contact in
                NavigationLink {
                    ContactDetail(contact: contact, write: { address in
                        dismiss()
                        write(address)
                    }, changed: { Task { contacts = await mail.contacts(query) } })
                } label: {
                    HStack(spacing: 12) {
                        Avatar(name: contact.name.isEmpty ? (contact.emails.first ?? "?") : contact.name, size: 36)
                        VStack(alignment: .leading) {
                            Text(contact.name.isEmpty ? (contact.emails.first ?? "") : contact.name)
                            if let email = contact.emails.first, !contact.name.isEmpty {
                                Text(email).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }
            .overlay {
                if contacts.isEmpty {
                    if syncing {
                        ProgressView(t("contacts_syncing"))
                    } else {
                        ContentUnavailableView(t("contacts_empty"), systemImage: "person.2")
                    }
                }
            }
            .searchable(text: $query, prompt: t("contacts_search"))
            .refreshable { await sync() }
            .navigationTitle(t("contacts"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button(t("close")) { dismiss() } }
                ToolbarItem(placement: .primaryAction) {
                    Button { Task { await sync() } } label: { Image(systemName: "arrow.clockwise") }
                        .accessibilityLabel(t("contacts_sync"))
                        .disabled(syncing)
                }
                ToolbarItem(placement: .primaryAction) {
                    Button {
                        editing = Contact(url: "", name: "", emails: [], phones: [], organization: "", note: "")
                    } label: { Image(systemName: "plus") }
                        .accessibilityLabel(t("contact_new"))
                }
            }
            .sheet(item: Binding(get: { editing.map(EditingContact.init) }, set: { editing = $0?.contact })) { item in
                NavigationStack {
                    ContactEditor(contact: item.contact) {
                        editing = nil
                        Task { contacts = await mail.contacts(query) }
                    }
                }
                .showsNotices()
            }
            .task(id: query) {
                contacts = await mail.contacts(query)
            }
        }
    }

    private func sync() async {
        syncing = true
        defer { syncing = false }
        do {
            let n = try await mail.syncContacts()
            notices.info(t("contacts_count", ["n": "\(n)"]))
        } catch {
            notices.error(describe(error))
        }
        contacts = await mail.contacts(query)
    }
}

struct EditingContact: Identifiable {
    let contact: Contact
    let id = UUID()
}

struct ContactDetail: View {
    let contact: Contact
    let write: (String) -> Void
    /// The contact was changed or deleted: the list reloads.
    var changed: () -> Void = {}

    @Environment(\.dismiss) private var dismiss
    @State private var editing = false

    var body: some View {
        List {
            Section {
                HStack(spacing: 16) {
                    Avatar(name: contact.name.isEmpty ? (contact.emails.first ?? "?") : contact.name, size: 56)
                    VStack(alignment: .leading) {
                        Text(contact.name).font(.title3.weight(.semibold))
                        if !contact.organization.isEmpty {
                            Text(contact.organization).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            if !contact.emails.isEmpty {
                Section(t("email")) {
                    ForEach(contact.emails, id: \.self) { email in
                        Button {
                            write(email)
                        } label: {
                            Label(email, systemImage: "envelope")
                        }
                    }
                }
            }
            if !contact.phones.isEmpty {
                Section {
                    ForEach(contact.phones, id: \.self) { phone in
                        let digits = phone.filter { $0.isNumber || $0 == "+" }
                        if let url = URL(string: "tel:\(digits)") {
                            Link(destination: url) { Label(phone, systemImage: "phone") }
                        }
                    }
                }
            }
            if !contact.note.isEmpty {
                Section { Text(contact.note).textSelection(.enabled) }
            }
        }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if !contact.url.isEmpty {
                Button(t("contact_edit")) { editing = true }
            }
        }
        .sheet(isPresented: $editing) {
            NavigationStack {
                ContactEditor(contact: contact) {
                    editing = false
                    changed()
                    dismiss()
                }
            }
            .showsNotices()
        }
    }
}

/// Writing a contact: name, addresses, phones, organization, note; and
/// deleting a stored one.
struct ContactEditor: View {
    @State var contact: Contact
    /// Saved or deleted.
    let done: () -> Void

    @Environment(MailRepository.self) private var mail
    @Environment(Notices.self) private var notices
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false
    @State private var confirmDelete = false

    var body: some View {
        Form {
            Section {
                TextField(t("contact_name"), text: $contact.name)
                TextField(t("contact_org"), text: $contact.organization)
            }
            Section(t("contact_emails")) {
                ForEach(contact.emails.indices, id: \.self) { i in
                    TextField("nome@esempio.it", text: $contact.emails[i])
                        .keyboardType(.emailAddress).textInputAutocapitalization(.never).autocorrectionDisabled()
                }
                .onDelete { contact.emails.remove(atOffsets: $0) }
                Button { contact.emails.append("") } label: { Label(t("contact_add_email"), systemImage: "plus") }
            }
            Section(t("contact_phones")) {
                ForEach(contact.phones.indices, id: \.self) { i in
                    TextField("+39", text: $contact.phones[i]).keyboardType(.phonePad)
                }
                .onDelete { contact.phones.remove(atOffsets: $0) }
                Button { contact.phones.append("") } label: { Label(t("contact_add_phone"), systemImage: "plus") }
            }
            Section(t("contact_note")) {
                TextField(t("contact_note"), text: $contact.note, axis: .vertical).lineLimit(2...6)
            }
            if !contact.url.isEmpty {
                Section {
                    Button(t("contact_delete"), role: .destructive) { confirmDelete = true }
                }
            }
        }
        .navigationTitle(t(contact.url.isEmpty ? "contact_new" : "contact_edit"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(t("cancel")) { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button(t("rules_save")) { save() }
                    .disabled(busy || (contact.name.trimmingCharacters(in: .whitespaces).isEmpty
                        && contact.emails.allSatisfy { $0.trimmingCharacters(in: .whitespaces).isEmpty }))
            }
        }
        .confirmationDialog(t("contact_delete"), isPresented: $confirmDelete, titleVisibility: .visible) {
            Button(t("contact_delete"), role: .destructive) { delete() }
        }
    }

    private func save() {
        busy = true
        var c = contact
        c.emails = c.emails.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        c.phones = c.phones.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        Task {
            defer { busy = false }
            do {
                try await mail.saveContact(c)
                notices.info(t("contact_saved"))
                done()
            } catch {
                notices.error(describe(error))
            }
        }
    }

    private func delete() {
        busy = true
        Task {
            defer { busy = false }
            do {
                try await mail.deleteContact(contact.url)
                notices.info(t("contact_deleted"))
                done()
            } catch {
                notices.error(describe(error))
            }
        }
    }
}
