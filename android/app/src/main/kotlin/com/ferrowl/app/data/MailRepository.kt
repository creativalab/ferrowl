package com.ferrowl.app.data

import com.ferrowl.app.push.PushRegistrar
import com.ferrowl.app.ui.notice.Notices
import com.ferrowl.core.Account
import com.ferrowl.core.AddressSuggestion
import com.ferrowl.core.CalendarEvent
import com.ferrowl.core.CalendarInfo
import com.ferrowl.core.Contact
import com.ferrowl.core.Engine
import com.ferrowl.core.EventDraft
import com.ferrowl.core.Folder
import com.ferrowl.core.FolderKind
import com.ferrowl.core.Header
import com.ferrowl.core.InboxPeek
import com.ferrowl.core.InviteReply
import com.ferrowl.core.Message
import com.ferrowl.core.Outgoing
import com.ferrowl.core.Rule
import com.ferrowl.core.RulesOutcome
import com.ferrowl.core.markdownToHtml
import com.ferrowl.core.oauthForget
import com.ferrowl.core.translate
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import javax.inject.Inject
import javax.inject.Named
import javax.inject.Singleton

/**
 * The single entry point to mail for the UI: it wraps the Rust [Engine]
 * and the saved account. ViewModels talk to this class, never to the
 * engine directly, so the rest of the app does not depend on the bindings.
 *
 * Engine calls are `suspend` functions: they run off the main thread and
 * throw `FerrowlException` with a message ready to show.
 */
@Singleton
class MailRepository @Inject constructor(
    private val engine: Engine,
    /** A second engine, to read the inbox of an account that is not the active one. */
    @Named("side") private val side: Engine,
    private val accounts: AccountStore,
    private val push: PushRegistrar,
    private val noticeBoard: Notices,
    private val accountUnread: AccountUnread,
) {
    private val connectLock = Mutex()
    private val _folders = MutableStateFlow<List<Folder>>(emptyList())
    private val _changes = MutableSharedFlow<Unit>(extraBufferCapacity = 8)

    /** Folders of the connected account (empty until [ensureConnected]). */
    val folders: StateFlow<List<Folder>> = _folders.asStateFlow()

    /** Emits after messages were moved, deleted or sent: lists reload. */
    val changes: SharedFlow<Unit> = _changes.asSharedFlow()

    private val _arrivals = MutableSharedFlow<String>(extraBufferCapacity = 8)

    /**
     * Emits an address when a push says it has new mail (or mail read
     * elsewhere): an open mailbox of that account fetches its inbox again.
     */
    val arrivals: SharedFlow<String> = _arrivals.asSharedFlow()

    fun arrived(email: String) {
        _arrivals.tryEmit(email)
    }

    private val _returns = MutableSharedFlow<Unit>(extraBufferCapacity = 1)

    /**
     * Emits when the app comes back to the screen: what changed meanwhile
     * elsewhere (mail deleted or read on another device) is fetched again.
     */
    val returns: SharedFlow<Unit> = _returns.asSharedFlow()

    fun returned() {
        _returns.tryEmit(Unit)
    }


    suspend fun savedAccount(): Account? = accounts.account.first()

    /** Every account on the phone, in the order they were added. */
    val accountList: Flow<List<Account>> = accounts.accounts

    /** The active account's address (null with no account). */
    val activeEmail: Flow<String?> = accounts.account.map { it?.email }.distinctUntilChanged()

    private val _switched = MutableSharedFlow<Unit>(extraBufferCapacity = 4)

    /** Emits after another account became the active one: screens start over. */
    val switched: SharedFlow<Unit> = _switched.asSharedFlow()

    /** Makes another account the active one and connects it. */
    suspend fun switchTo(email: String) {
        connectLock.withLock {
            val account = accounts.accounts.first().firstOrNull { it.email.equals(email, ignoreCase = true) } ?: return
            _folders.value = engine.connect(account)
            accounts.setActive(account.email)
        }
        _switched.tryEmit(Unit)
        learnFromSent()
        syncContactsQuietly()
        // Notifications follow the active account.
        runCatching { push.accountChanged() }
    }

    /**
     * Without Ferrowl Pro only the first account may be used: makes it the
     * active one again. It is saved before connecting, so if the connection
     * fails the next attempt (any server action) uses the right account.
     */
    suspend fun backToFirst(email: String) {
        connectLock.withLock {
            accounts.setActive(email)
            _folders.value = emptyList()
        }
        _switched.tryEmit(Unit)
        runCatching { push.accountChanged() }
        ensureConnected()
    }

    /**
     * Removes an account from the phone (its sign-in too); another one, if
     * any, becomes active. Returns false when no account is left.
     */
    suspend fun removeAccount(email: String): Boolean {
        val all = accounts.accounts.first()
        val removed = all.firstOrNull { it.email.equals(email, ignoreCase = true) } ?: return true
        removed.oauth?.let { oauthForget(it, removed.email) }
        // Cached mail, search index, contacts and calendars of the account
        // (shared when the same mailbox is added twice: kept then).
        val sharesCache = all.any {
            it !== removed && it.username == removed.username && it.incomingHost == removed.incomingHost
        }
        if (!sharesCache) withContext(Dispatchers.IO) { engine.forgetAccount(removed) }
        val next = accounts.remove(removed.email)
        accountUnread.forget(removed.email)
        runCatching { push.accountChanged() }
        if (next == null) {
            _folders.value = emptyList()
            return false
        }
        connectLock.withLock { _folders.value = engine.connect(next) }
        _switched.tryEmit(Unit)
        return true
    }

    /**
     * Saves an account's name and contacts/calendar servers. The account in
     * use connects again, so its address book and calendars follow them.
     */
    suspend fun updateAccount(account: Account) {
        accounts.update(account)
        if (account.email.equals(savedAccount()?.email, ignoreCase = true)) {
            connectLock.withLock { _folders.value = engine.connect(account) }
            syncContactsQuietly()
        }
    }

    /** Checks settings without saving them. Returns a confirmation text. */
    suspend fun test(account: Account): String = engine.test(account)

    /** Connects and, if it works, saves the account. */
    suspend fun setUp(account: Account) {
        connectLock.withLock {
            _folders.value = engine.connect(account)
            accounts.save(account)
        }
        learnFromSent()
        syncContactsQuietly()
        // Notifications: the server gets this account (again, with its new settings).
        runCatching { push.accountSaved(account.email) }
    }

    /**
     * Before an action on the server: the list can be on screen before the
     * connection is made (it comes from the phone's copy), so wait for it.
     */
    private suspend fun ready() {
        if (_folders.value.isEmpty()) ensureConnected()
    }

    /** Connects the saved account once per process. */
    suspend fun ensureConnected(): Boolean {
        val connected = connectLock.withLock {
            if (_folders.value.isNotEmpty()) return true
            val account = accounts.account.first() ?: return false
            _folders.value = engine.connect(account)
            true
        }
        if (connected) {
            learnFromSent()
            syncContactsQuietly()
            // Notifications on: tell the server again (it forgets the key on restart).
            runCatching { push.sync() }
        }
        return connected
    }

    /**
     * The address book learns who we write to from the Sent folder: its
     * newest page is read once per connection. Failures don't matter here.
     */
    private suspend fun learnFromSent() {
        val sent = folderOf(FolderKind.SENT) ?: return
        runCatching { engine.headers(sent.id) }
    }

    /**
     * Contacts and calendars from the account's CardDAV/CalDAV servers,
     * once per connection.
     * Most mail accounts have no such server: failures are not shown here
     * (the contacts screen shows them when the user syncs by hand).
     */
    private suspend fun syncContactsQuietly() {
        runCatching { engine.syncContacts() }
        runCatching { engine.syncCalendar() }
    }

    /** Event occurrences between two instants (seconds since the epoch). */
    suspend fun agenda(from: Long, to: Long): List<CalendarEvent> =
        withContext(Dispatchers.IO) { engine.agenda(from, to) }

    /** The account's calendars. */
    suspend fun calendars(): List<CalendarInfo> = withContext(Dispatchers.IO) { engine.calendars() }

    /** The form for a stored event. */
    suspend fun eventDraft(url: String): EventDraft? = withContext(Dispatchers.IO) { engine.eventDraft(url) }

    /** Saves a new or changed event; returns the text to show. */
    suspend fun saveEvent(calendarUrl: String, url: String?, draft: EventDraft): String =
        engine.saveEvent(calendarUrl, url, draft)

    /** Deletes a stored event; returns the text to show. */
    suspend fun deleteEvent(url: String): String {
        ready()
        return engine.deleteEvent(url)
    }

    /** Updates the calendars from the server. */
    suspend fun syncCalendar() {
        engine.syncCalendar()
    }

    /** Saved contacts matching [query] (all when empty), by name. */
    suspend fun contacts(query: String): List<Contact> =
        withContext(Dispatchers.IO) { engine.contacts(query) }

    /** Saves a contact written in the app (on the contacts server when the account has one, else on the phone). */
    suspend fun saveContact(contact: Contact) {
        ready()
        engine.saveContact(contact)
    }

    suspend fun deleteContact(url: String) {
        ready()
        engine.deleteContact(url)
    }

    /** Updates the contacts from the server; returns how many are saved. */
    suspend fun syncContacts(): Int = engine.syncContacts().toInt()

    /**
     * Answers an invitation: mails the reply to the organizer and updates
     * the calendar when the account has one. Returns the text to show.
     */
    suspend fun respondInvitation(calendar: String, reply: InviteReply): String =
        engine.respondInvitation(calendar, reply)

    /** Our earlier answer to the invitation with this event UID, if any. */
    suspend fun invitationReply(uid: String): InviteReply? =
        withContext(Dispatchers.IO) { engine.invitationReply(uid) }

    /** Whether the event is in the calendar. */
    suspend fun inCalendar(uid: String): Boolean = withContext(Dispatchers.IO) { engine.inCalendar(uid) }

    /** Removes a cancelled event from the calendar; returns the text to show. */
    suspend fun removeCancelled(uid: String): String {
        ready()
        return engine.removeCancelled(uid)
    }

    /** Recipients matching what is being typed. */
    suspend fun suggest(query: String): List<AddressSuggestion> =
        withContext(Dispatchers.IO) { engine.suggestAddresses(query) }

    fun folder(id: String): Folder? = _folders.value.firstOrNull { it.id == id }

    private fun folderOf(kind: FolderKind): Folder? = _folders.value.firstOrNull { it.kind == kind }

    fun inboxId(): String = folderOf(FolderKind.INBOX)?.id ?: "INBOX"

    /**
     * The active account's inbox as saved on the phone, before connecting
     * (shown at once while the connection is made). Also says whether it is
     * a Gmail account (its inbox has tabs).
     */
    suspend fun cachedInbox(): Pair<List<Header>, Boolean> = withContext(Dispatchers.IO) {
        val account = accounts.account.first() ?: return@withContext emptyList<Header>() to false
        engine.cachedInbox(account) to account.incomingHost.endsWith("gmail.com")
    }

    private val sideLock = Mutex()

    /**
     * For a new-mail notification: the unread messages of [email]'s inbox
     * above UID [after] (at most [limit]) and how many there are, read on a
     * connection of its own (no folder list, no cache, no previews), so it
     * fits in the time Android gives a push. The account on screen stays
     * connected. Null for an account no longer on the phone.
     */
    suspend fun peekInbox(email: String, after: Long, limit: Int): InboxPeek? {
        val account = accounts.accounts.first().firstOrNull { it.email.equals(email, ignoreCase = true) } ?: return null
        return engine.peekInbox(account, after.toUInt(), limit.toUInt())
    }

    /**
     * Search in what the phone keeps of any account (not only the active
     * one), newest first: for other apps (Ferrowl Bridge).
     */
    suspend fun searchAccount(account: Account, query: String, limit: Int): List<Header> =
        withContext(Dispatchers.IO) { engine.searchAccount(account, "INBOX", query, limit.toUInt()) }

    /** A whole message of any account, without marking it read (other apps). */
    suspend fun peekMessage(account: Account, folderId: String, uid: UInt): Message =
        engine.peekMessage(account, folderId, uid)

    /** The synced agenda of any account, between two Unix times (other apps). */
    suspend fun agendaFor(account: Account, from: Long, to: Long): List<CalendarEvent> =
        withContext(Dispatchers.IO) { engine.agendaFor(account, from, to) }

    /** The address book of any account (other apps). */
    suspend fun contactsFor(account: Account, query: String): List<Contact> =
        withContext(Dispatchers.IO) { engine.contactsFor(account, query) }

    /** Messages already on the phone (local database, off the main thread). */
    suspend fun cachedHeaders(folderId: String): List<Header> =
        withContext(Dispatchers.IO) { engine.cachedHeaders(folderId) }

    /** Newest messages from the server. */
    suspend fun headers(folderId: String): List<Header> {
        ready()
        return engine.headers(folderId).headers
    }

    suspend fun search(folderId: String, query: String): List<Header> =
        withContext(Dispatchers.IO) { engine.search(folderId, query) }

    /** Unread count of every folder. */
    suspend fun unread(): Map<String, UInt> =
        ready().let { _ -> engine.unread(_folders.value.filter { it.selectable }.map { it.id }) }

    suspend fun message(folderId: String, uid: UInt): Message {
        ready()
        return engine.message(folderId, uid)
    }

    suspend fun setSeen(folderId: String, uid: UInt, seen: Boolean) {
        ready()
        engine.setSeen(folderId, uid, seen)
        _changes.emit(Unit)
    }

    suspend fun archive(folderId: String, uid: UInt) {
        ready()
        engine.archive(folderId, uid)
        changed(translate("archived"))
    }

    /** Whether deleting from this folder is final (it is the Trash). */
    fun deletesForever(folderId: String): Boolean =
        folder(folderId)?.kind == FolderKind.TRASH || folderOf(FolderKind.TRASH) == null

    /** Moves to the Trash, or deletes for good when already there. */
    suspend fun delete(folderId: String, uid: UInt) {
        ready()
        if (deletesForever(folderId)) {
            engine.deleteForever(folderId, uid)
            changed(translate("deleted_forever"))
        } else {
            engine.moveTo(folderId, uid, folderOf(FolderKind.TRASH)!!.id)
            changed(translate("moved_to_trash"))
        }
    }

    /** Whether the account is Gmail (messages have labels). */
    fun isGmail(): Boolean = engine.isGmail()

    /** Gmail labels that can be put on messages of [folderId]: the user's own. */
    fun labelChoices(folderId: String): List<Folder> = _folders.value.filter {
        it.kind == FolderKind.OTHER && it.selectable && it.id != folderId &&
            !it.id.startsWith("[Gmail]") && !it.id.startsWith("[Google Mail]")
    }

    /** Adds or removes a Gmail label (a folder id) on messages. */
    suspend fun setLabel(folderId: String, uids: List<UInt>, label: String, on: Boolean) {
        ready()
        engine.setLabelMany(folderId, uids, label, on)
    }

    /** Moves messages to Spam, or back to the inbox ([toSpam] false). */
    suspend fun spam(folderId: String, uids: List<UInt>, toSpam: Boolean) {
        ready()
        if (toSpam) engine.reportSpam(folderId, uids) else engine.notSpam(folderId, uids)
        val key = if (toSpam) "spam_moved" else "inbox_moved"
        changed(translate(key).replace("{n}", uids.size.toString()))
    }

    /** Whether a folder is the Spam folder. */
    fun isSpam(folderId: String): Boolean = folder(folderId)?.kind == FolderKind.SPAM

    /** Blocks (false) or allows (true) a sender: an address or "@domain". */
    suspend fun setSenderRule(address: String, allow: Boolean) =
        withContext(Dispatchers.IO) { engine.setSenderRule(address, allow) }

    suspend fun removeSenderRule(address: String) = withContext(Dispatchers.IO) { engine.removeSenderRule(address) }

    suspend fun senderRules(allow: Boolean): List<String> = withContext(Dispatchers.IO) { engine.senderRules(allow) }

    /**
     * Applies the sender lists to a freshly loaded folder; returns the
     * messages moved (to drop from the list).
     */
    suspend fun applySenderRules(folderId: String, headers: List<Header>): List<UInt> {
        val moved = engine.applySenderRules(folderId, headers)
        if (moved.isNotEmpty()) noticeBoard.info(translate("rules_moved").replace("{n}", moved.size.toString()))
        return moved
    }

    /** The inbox's new messages through the user's rules (each once). */
    suspend fun applyRules(folderId: String, headers: List<Header>): RulesOutcome? {
        if (folderId != inboxId() || headers.isEmpty()) return null
        val outcome = runCatching { engine.applyRules(folderId, headers) }.getOrNull() ?: return null
        val touched = (outcome.removed + outcome.read + outcome.starred).toSet()
        if (touched.isNotEmpty()) noticeBoard.info(translate("rules_applied").replace("{n}", touched.size.toString()))
        return outcome
    }

    /** The active account's rules. */
    suspend fun rules(): List<Rule> = savedAccount()?.let { engine.rules(it) }.orEmpty()

    suspend fun saveRule(rule: Rule) {
        savedAccount()?.let { engine.saveRule(it, rule) }
    }

    suspend fun deleteRule(id: String) {
        savedAccount()?.let { engine.deleteRule(it, id) }
    }

    /** The inbox messages on the phone a condition picks (a rule's preview). */
    suspend fun rulePreview(condition: String): List<Header> = withContext(Dispatchers.IO) {
        savedAccount()?.let { engine.rulePreview(it, condition, 5u) }.orEmpty()
    }

    /** Which of these messages of [email] a rule keeps quiet. */
    suspend fun silenced(email: String, headers: List<Header>): Set<UInt> {
        val account = accounts.accounts.first().firstOrNull { it.email.equals(email, ignoreCase = true) } ?: return emptySet()
        return withContext(Dispatchers.IO) { engine.silenced(account, headers).toSet() }
    }

    /** Stars or unstars messages (IMAP \\Flagged, shown by every client). */
    suspend fun setFlagged(folderId: String, uids: List<UInt>, flagged: Boolean) {
        ready()
        engine.setFlaggedMany(folderId, uids, flagged)
    }

    /** Marks several messages read or unread at once. */
    suspend fun setSeen(folderId: String, uids: List<UInt>, seen: Boolean) {
        ready()
        engine.setSeenMany(folderId, uids, seen)
        _changes.emit(Unit)
    }

    /** Archives messages; `quiet` when the screen already confirmed it (swipe). */
    suspend fun archive(folderId: String, uids: List<UInt>, quiet: Boolean = false) {
        ready()
        engine.archiveMany(folderId, uids)
        changed(if (quiet) "" else translate("bulk_archived").replace("{n}", uids.size.toString()))
    }

    /** Moves several messages to another folder. */
    suspend fun move(folderId: String, uids: List<UInt>, targetId: String) {
        ready()
        engine.moveMany(folderId, uids, targetId)
        changed(translate("bulk_moved").replace("{n}", uids.size.toString()))
    }

    /**
     * Moves several messages to the Trash, or deletes them for good when
     * already there; `quiet` when the screen already confirmed it (swipe).
     */
    suspend fun delete(folderId: String, uids: List<UInt>, quiet: Boolean = false) {
        ready()
        val n = uids.size.toString()
        if (deletesForever(folderId)) {
            engine.deleteForeverMany(folderId, uids)
            changed(if (quiet) "" else translate("bulk_deleted_forever").replace("{n}", n))
        } else {
            engine.moveMany(folderId, uids, folderOf(FolderKind.TRASH)!!.id)
            changed(if (quiet) "" else translate("bulk_trashed").replace("{n}", n))
        }
    }

    /**
     * Sends; Markdown formatting typed by the user also goes out as HTML,
     * as on desktop. A warning is shown when the copy in Sent failed.
     */
    suspend fun send(draft: Outgoing) {
        ready()
        val warning = engine.send(draft.copy(html = markdownToHtml(draft.body)))
        changed(warning?.let { translate("sent_no_copy").replace("{err}", it) } ?: translate("sent_ok"))
    }

    private suspend fun changed(notice: String) {
        _changes.emit(Unit)
        noticeBoard.info(notice)
    }
}
