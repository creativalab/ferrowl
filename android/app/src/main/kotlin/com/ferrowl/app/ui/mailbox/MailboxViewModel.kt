package com.ferrowl.app.ui.mailbox

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MailRepository
import com.ferrowl.app.data.SettingsStore
import com.ferrowl.app.push.NotificationOpen
import com.ferrowl.app.ui.notice.Notices
import com.ferrowl.core.Conversation
import com.ferrowl.core.FerrowlException
import com.ferrowl.core.Folder
import com.ferrowl.core.Header
import com.ferrowl.core.SortOrder
import com.ferrowl.core.groupConversations
import com.ferrowl.core.sortConversations
import com.ferrowl.core.translate
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class MailboxState(
    /** Addresses of every account on the phone, and the active one. */
    val accounts: List<String> = emptyList(),
    val active: String = "",
    /** Ferrowl Pro (more than one account) and its yearly price, once known. */
    val pro: Boolean = false,
    val proPrice: String? = null,
    /** The Ferrowl Pro offer is on screen. */
    val showPro: Boolean = false,
    /** Ferrowl Pro was just bought from "Add account": open that screen once. */
    val openAddAccount: Boolean = false,
    val folders: List<Folder> = emptyList(),
    val unread: Map<String, UInt> = emptyMap(),
    val folderId: String = "",
    val messages: List<Header> = emptyList(),
    val refreshing: Boolean = false,
    /** Another account is being connected (shown over the screen). */
    val switching: String? = null,
    val error: String? = null,
    /** Search text; when not blank the list shows [results]. */
    val query: String = "",
    val searching: Boolean = false,
    val results: List<Header> = emptyList(),
    /** Conversations (true) or one row per message. */
    val threaded: Boolean = true,
    val sort: SortOrder = SortOrder.NEWEST,
    /** Gmail inbox: the tabs are shown, and which one is open (0 = Primary). */
    val tabs: Boolean = false,
    val category: Int = 0,
    /** Checked messages (uids); not empty = selection mode. */
    val selected: Set<UInt> = emptySet(),
    /** Asking before deleting the selection for good (in the Trash). */
    val confirmDelete: Boolean = false,
) {
    val selecting: Boolean get() = selected.isNotEmpty()

    /** Gmail inbox: unread messages per tab (category index → count). */
    val unreadByCategory: Map<Int, Int>
        get() = if (!tabs) emptyMap() else messages.filter { !it.seen }.groupingBy { it.category.toInt() }.eachCount()
    val folder: Folder? get() = folders.firstOrNull { it.id == folderId }
    val shown: List<Header>
        get() = when {
            query.isNotBlank() -> results
            tabs -> messages.filter { it.category.toInt() == category }
            else -> messages
        }

    /** The list rows: whole conversations, or single messages as one-member ones. */
    val rows: List<Conversation>
        get() = sortConversations(
            if (threaded) {
                groupConversations(shown)
            } else {
                shown.map { Conversation(it, listOf(it.uid), if (it.seen) 0u else 1u) }
            },
            sort,
        )
}

@HiltViewModel
class MailboxViewModel @Inject constructor(
    private val repo: MailRepository,
    private val settings: SettingsStore,
    private val notices: Notices,
    private val billing: Billing,
    private val notificationOpen: NotificationOpen,
    private val push: com.ferrowl.app.push.PushRegistrar,
    private val accountUnread: com.ferrowl.app.data.AccountUnread,
) : ViewModel() {
    /** Each account's unread count, for the drawer. */
    val accountCounts = accountUnread.counts

    private val _state = MutableStateFlow(MailboxState())
    val state: StateFlow<MailboxState> = _state.asStateFlow()

    /** The error was shown to the user (as a notice at the top). */
    fun errorShown() = _state.update { it.copy(error = null) }

    private var searchJob: Job? = null

    init {
        viewModelScope.launch { settings.threaded.collect { on -> _state.update { it.copy(threaded = on) } } }
        viewModelScope.launch { settings.sort.collect { o -> _state.update { it.copy(sort = o) } } }
        viewModelScope.launch {
            // The saved inbox right away; connecting can take a while.
            _state.update { it.copy(refreshing = true) }
            val (early, gmail) = repo.cachedInbox()
            if (early.isNotEmpty()) {
                _state.update { if (it.folderId.isEmpty()) it.copy(folderId = "INBOX", messages = early, tabs = gmail) else it }
            }
            try {
                repo.ensureConnected()
                _state.update {
                    it.copy(folders = repo.folders.value, folderId = repo.inboxId(), tabs = repo.isGmail())
                }
                refresh()
            } catch (e: FerrowlException) {
                _state.update { it.copy(refreshing = false, error = e.message) }
            }
        }
        viewModelScope.launch {
            repo.accountList.collect { list -> _state.update { it.copy(accounts = list.map { a -> a.email }) } }
        }
        viewModelScope.launch { repo.activeEmail.collect { e -> _state.update { it.copy(active = e.orEmpty()) } } }
        viewModelScope.launch { billing.price.collect { p -> _state.update { it.copy(proPrice = p) } } }
        viewModelScope.launch {
            billing.pro.collect { on ->
                _state.update { it.copy(pro = on, showPro = if (on) false else it.showPro) }
                val wanted = buying
                if (on && wanted != null) {
                    buying = null
                    notices.info(translate("pro_thanks"))
                    // Carry on with what the user wanted to do.
                    if (wanted == ADD) _state.update { it.copy(openAddAccount = true) } else switchAccount(wanted)
                }
            }
        }
        // Without Ferrowl Pro only the first account (the free one) may stay
        // active: checked whenever Pro, the accounts or the active one change,
        // once the account list is known.
        viewModelScope.launch {
            combine(billing.pro, repo.accountList, repo.activeEmail) { on, list, active ->
                if (on) null else list.firstOrNull()?.email?.takeIf { first -> active != null && !active.equals(first, ignoreCase = true) }
            }.distinctUntilChanged().collect { first ->
                if (first == null) return@collect
                try {
                    repo.backToFirst(first)
                } catch (e: FerrowlException) {
                    _state.update { it.copy(refreshing = false, error = e.message) }
                }
            }
        }
        // Another account became active: start over with its inbox.
        viewModelScope.launch {
            repo.switched.collect {
                _state.update {
                    it.copy(
                        folders = repo.folders.value,
                        folderId = repo.inboxId(),
                        tabs = repo.isGmail(),
                        unread = emptyMap(),
                        messages = emptyList(),
                        query = "",
                        results = emptyList(),
                        selected = emptySet(),
                        category = 0,
                        error = null,
                    )
                }
                refresh()
            }
        }
        // A push for the account on screen (new mail, or mail read
        // elsewhere): its inbox, if open, comes again from the server.
        viewModelScope.launch {
            repo.arrivals.collect { email ->
                val s = _state.value
                if (!email.equals(s.active, ignoreCase = true)) return@collect
                if (s.folderId == repo.inboxId() && s.query.isBlank()) refresh(quiet = true) else loadUnread()
            }
        }
        // Back on screen: changes made elsewhere meanwhile (mail deleted or
        // read on the computer) show without pulling to refresh.
        viewModelScope.launch {
            repo.returns.collect {
                if (_state.value.query.isBlank()) refresh(quiet = true) else loadUnread()
            }
        }
        // Something was moved, deleted or sent: reload from the local copy.
        viewModelScope.launch {
            repo.changes.collect {
                val id = _state.value.folderId
                if (id.isNotEmpty()) _state.update { it.copy(messages = repo.cachedHeaders(id)) }
                loadUnread()
            }
        }
    }

    /** Instant notifications: the first-launch question is on screen. */
    private val _askPush = MutableStateFlow(false)
    val askPush: StateFlow<Boolean> = _askPush.asStateFlow()

    init {
        viewModelScope.launch {
            if (push.needsAsking() && repo.savedAccount() != null) _askPush.value = true
        }
    }

    /** "Turn on": registers with the push server (after the system permission). */
    fun enablePush() {
        _askPush.value = false
        viewModelScope.launch {
            try {
                push.enable()
            } catch (e: Exception) {
                notices.error(translate("push_error").replace("{err}", e.message.orEmpty()))
                runCatching { push.disable() }
            }
        }
    }

    /** "Not now": nothing goes to the server; Settings can turn it on later. */
    fun declinePush() {
        _askPush.value = false
        viewModelScope.launch { runCatching { push.disable() } }
    }

    /** A tapped notification asks for this message (switch account first). */
    val openRequests = notificationOpen.pending

    /**
     * Handles a tapped notification: switches to its account (the request
     * stays until that account is active), then returns the inbox and the
     * message uid to open, or null when there is nothing to open yet.
     */
    suspend fun takeOpenRequest(): Pair<String, UInt?>? {
        val r = notificationOpen.pending.value ?: return null
        val s = _state.value
        if (!r.email.equals(s.active, ignoreCase = true)) {
            if (s.accounts.any { it.equals(r.email, ignoreCase = true) }) switchAccount(r.email) else notificationOpen.done()
            return null
        }
        notificationOpen.done()
        repo.ensureConnected()
        openFolder(repo.inboxId())
        return repo.inboxId() to r.uid?.toUInt()
    }

    /**
     * Whether another account may be added; without Ferrowl Pro only the
     * first one is free, and the offer is shown instead.
     */
    fun mayAddAccount(): Boolean {
        val s = _state.value
        if (s.pro || s.accounts.isEmpty()) return true
        offerPro(ADD)
        return false
    }

    /** What the user wanted when the offer appeared: [ADD] or an account's address. */
    private var offeredFor: String? = null

    /** Purchase sheet open for this wish; done once Pro arrives. */
    private var buying: String? = null

    private fun offerPro(wanted: String) {
        offeredFor = wanted
        billing.start() // fresh price, and maybe the user is Pro already
        _state.update { it.copy(showPro = true) }
    }

    fun dismissPro() = _state.update { it.copy(showPro = false) }

    /** Google Play's purchase sheet opened. */
    fun purchaseStarted() {
        buying = offeredFor
        _state.update { it.copy(showPro = false) }
    }

    fun addAccountOpened() = _state.update { it.copy(openAddAccount = false) }

    /** The subscription could not be offered (not set up in Google Play yet, offline). */
    fun proUnavailable() {
        _state.update { it.copy(showPro = false) }
        notices.error(translate("pro_unavailable"))
    }

    /** Shows another account (connects it first). Accounts after the first need Ferrowl Pro. */
    fun switchAccount(email: String) {
        if (email.equals(_state.value.active, ignoreCase = true)) return
        val first = _state.value.accounts.firstOrNull()
        if (!_state.value.pro && first != null && !email.equals(first, ignoreCase = true)) {
            offerPro(email)
            return
        }
        _state.update { it.copy(refreshing = true, switching = email, error = null) }
        viewModelScope.launch {
            try {
                repo.switchTo(email)
            } catch (e: FerrowlException) {
                _state.update { it.copy(refreshing = false, error = e.message) }
            } finally {
                _state.update { it.copy(switching = null) }
            }
        }
    }

    fun openFolder(id: String) {
        if (id == _state.value.folderId) return
        _state.update {
            it.copy(
                folderId = id,
                messages = emptyList(),
                query = "",
                results = emptyList(),
                selected = emptySet(),
                tabs = repo.isGmail() && id == repo.inboxId(),
                category = 0,
            )
        }
        refresh()
    }

    /**
     * The folder being fetched from the server, and whether another fetch was
     * asked meanwhile: fetches never overlap (each applies the rules, which
     * must not run twice on the same new messages). Main thread only.
     */
    private var fetching: String? = null
    private var fetchAgain = false

    /**
     * The local copy at once, then the newest messages from the server.
     * [quiet]: not asked by the user (a push, the app back on screen), so no
     * error is shown when offline.
     */
    fun refresh(quiet: Boolean = false) {
        val id = _state.value.folderId.ifEmpty { return }
        if (fetching == id) {
            fetchAgain = true
            return
        }
        fetching = id
        _state.update { it.copy(refreshing = true, error = if (quiet) it.error else null) }
        viewModelScope.launch {
            try {
                fetch(id, quiet)
                // Asked again while fetching (a push during the fetch): once more.
                while (fetchAgain && _state.value.folderId == id) {
                    fetchAgain = false
                    fetch(id, quiet = true)
                }
            } finally {
                fetchAgain = false
                if (fetching == id) fetching = null
                _state.update { if (it.folderId == id) it.copy(refreshing = false) else it }
            }
        }
    }

    private suspend fun fetch(id: String, quiet: Boolean) {
        try {
            val cached = repo.cachedHeaders(id)
            if (cached.isNotEmpty()) _state.update { if (it.folderId == id) it.copy(messages = cached) else it }
            var fresh = repo.headers(id)
            // Sender lists: blocked mail leaves the inbox, allowed mail leaves Spam.
            val moved = runCatching { repo.applySenderRules(id, fresh) }.getOrDefault(emptyList())
            if (moved.isNotEmpty()) fresh = fresh.filter { it.uid !in moved }
            // The user's rules on the inbox's new messages.
            repo.applyRules(id, fresh)?.let { done ->
                fresh = fresh.filter { it.uid !in done.removed }.map {
                    it.copy(seen = it.seen || it.uid in done.read, flagged = it.flagged || it.uid in done.starred)
                }
            }
            _state.update { if (it.folderId == id) it.copy(messages = fresh) else it }
            loadUnread()
        } catch (e: FerrowlException) {
            if (!quiet) _state.update { it.copy(error = e.message) }
        }
    }

    private suspend fun loadUnread() {
        runCatching { repo.unread() }.onSuccess { counts ->
            _state.update { it.copy(unread = counts) }
            activeUnread()?.let { n ->
                // The inbox's unread messages for the widget, when it is the open folder.
                val s = _state.value
                val inbox = if (s.folderId == repo.inboxId()) s.messages.filter { !s.tabs || it.category.toInt() == 0 } else null
                accountUnread.set(s.active, n, recent = inbox)
            }
        }
    }

    /**
     * The account in use's unread count, as the drawer shows it: Gmail
     * counts the Primary tab (known while its inbox is open), the others
     * the server's inbox count.
     */
    fun activeUnread(): Int? {
        val s = _state.value
        if (s.active.isBlank()) return null
        val inbox = repo.inboxId()
        return when {
            !repo.isGmail() -> s.unread[inbox]?.toInt()
            s.folderId == inbox && s.tabs -> s.unreadByCategory[0] ?: 0
            else -> null
        }
    }

    fun setQuery(text: String) {
        _state.update { it.copy(query = text) }
        searchJob?.cancel()
        if (text.isBlank()) return
        searchJob = viewModelScope.launch {
            _state.update { it.copy(searching = true) }
            val found = repo.search(_state.value.folderId, text)
            _state.update { it.copy(results = found, searching = false) }
        }
    }

    /** Checks or unchecks a row (all messages of a conversation). */
    fun toggle(uids: List<UInt>) = _state.update { s ->
        val all = s.selected.containsAll(uids)
        s.copy(selected = if (all) s.selected - uids.toSet() else s.selected + uids)
    }

    enum class Pick { ALL, NONE, READ, UNREAD }

    fun select(pick: Pick) = _state.update { s ->
        s.copy(
            selected = s.shown.filter {
                when (pick) {
                    Pick.ALL -> true
                    Pick.NONE -> false
                    Pick.READ -> it.seen
                    Pick.UNREAD -> !it.seen
                }
            }.map { it.uid }.toSet(),
        )
    }

    fun clearSelection() = _state.update { it.copy(selected = emptySet(), confirmDelete = false) }

    fun markSelected(seen: Boolean) = bulk { folder, uids ->
        _state.update { s ->
            s.copy(messages = s.messages.map { if (it.uid in uids) it.copy(seen = seen) else it })
        }
        repo.setSeen(folder, uids, seen)
    }

    fun starSelected(on: Boolean) = bulk { folder, uids -> star(folder, uids, on) }

    /** The star of a list row (all its messages). */
    fun toggleStar(uids: List<UInt>, on: Boolean) {
        val folder = _state.value.folderId
        viewModelScope.launch {
            try {
                star(folder, uids, on)
            } catch (e: FerrowlException) {
                star(folder, uids, !on, local = true)
                _state.update { it.copy(error = e.message) }
            }
        }
    }

    /** Updates the list right away, then the server (unless [local]). */
    private suspend fun star(folder: String, uids: List<UInt>, on: Boolean, local: Boolean = false) {
        _state.update { s ->
            s.copy(
                messages = s.messages.map { if (it.uid in uids) it.copy(flagged = on) else it },
                results = s.results.map { if (it.uid in uids) it.copy(flagged = on) else it },
            )
        }
        if (!local) repo.setFlagged(folder, uids, on)
    }

    /** Gmail: whether the folder's messages have labels. */
    val gmail: Boolean get() = repo.isGmail()

    fun labelChoices(): List<Folder> = repo.labelChoices(_state.value.folderId)

    /** Adds or removes a label on the checked messages (the selection stays). */
    fun toggleLabel(label: String, on: Boolean) {
        val s = _state.value
        val uids = s.selected.toList()
        if (uids.isEmpty()) return
        val apply = { add: Boolean ->
            _state.update { st ->
                fun fix(h: Header) = if (h.uid in uids) h.copy(labels = h.labels.filter { it != label } + if (add) listOf(label) else emptyList()) else h
                st.copy(messages = st.messages.map(::fix), results = st.results.map(::fix))
            }
        }
        apply(on)
        viewModelScope.launch {
            try {
                repo.setLabel(s.folderId, uids, label, on)
            } catch (e: FerrowlException) {
                apply(!on)
                _state.update { it.copy(error = e.message) }
            }
        }
    }

    /** Gmail inbox tab (0 Primary … 4 Forums). */
    fun setCategory(c: Int) = _state.update { it.copy(category = c, selected = emptySet()) }

    fun setSort(order: SortOrder) {
        viewModelScope.launch { settings.setSort(order) }
    }

    /** Whether the open folder is Spam (the action becomes "Not spam"). */
    val inSpam: Boolean get() = repo.isSpam(_state.value.folderId)

    /** Report spam, or in Spam "not spam" (which also allows the senders). */
    fun spamSelected() {
        val toSpam = !inSpam
        val s = _state.value
        if (!toSpam) {
            val senders = s.shown.filter { it.uid in s.selected && it.fromAddress.isNotBlank() }.map { it.fromAddress }.toSet()
            viewModelScope.launch { senders.forEach { runCatching { repo.setSenderRule(it, true) } } }
        }
        bulk { folder, uids -> repo.spam(folder, uids, toSpam) }
    }

    fun archiveSelected() = bulk { folder, uids -> repo.archive(folder, uids) }

    /** Whether deleting in the open folder is for good (the Trash): no swipe there. */
    val deletesForever: Boolean get() = repo.deletesForever(_state.value.folderId)

    /**
     * Swipe or the trash button of an open message: the row leaves the list
     * at once and "Moved to Trash · Undo" shows; the server is asked only
     * when the notice is gone, so Undo really undoes it.
     */
    fun deleteRow(uids: List<UInt>) {
        val folder = _state.value.folderId
        val rows = _state.value.messages.filter { it.uid in uids }
        _state.update { s -> s.copy(messages = s.messages.filterNot { it.uid in uids }) }
        val job = viewModelScope.launch {
            delay(UNDO_MS)
            try {
                repo.delete(folder, uids, quiet = true)
            } catch (e: FerrowlException) {
                _state.update { it.copy(error = e.message) }
                refresh()
            }
        }
        notices.withAction(translate("moved_to_trash"), translate("undo")) {
            job.cancel()
            _state.update { s ->
                if (s.folderId != folder) s else s.copy(messages = (s.messages + rows).sortedByDescending { it.timestamp })
            }
        }
    }

    fun moveSelected(targetId: String) = bulk { folder, uids -> repo.move(folder, uids, targetId) }

    /** Trash, or (in the Trash) ask first and then delete for good. */
    fun deleteSelected(confirmed: Boolean = false) {
        val s = _state.value
        if (!confirmed && repo.deletesForever(s.folderId)) {
            _state.update { it.copy(confirmDelete = true) }
            return
        }
        bulk { folder, uids -> repo.delete(folder, uids) }
    }

    private fun bulk(action: suspend (folder: String, uids: List<UInt>) -> Unit) {
        val s = _state.value
        val uids = s.selected.toList()
        if (uids.isEmpty()) return
        _state.update { it.copy(selected = emptySet(), confirmDelete = false) }
        viewModelScope.launch {
            try {
                action(s.folderId, uids)
            } catch (e: FerrowlException) {
                _state.update { it.copy(error = e.message) }
            }
        }
    }

    /** Marks a message read in the list as soon as it is opened. */
    fun opened(uid: UInt) = _state.update { s ->
        s.copy(
            messages = s.messages.map { if (it.uid == uid) it.copy(seen = true) else it },
            results = s.results.map { if (it.uid == uid) it.copy(seen = true) else it },
        )
    }

    private companion object {
        /** [offeredFor] value: the user wanted to add an account. */
        private const val ADD = "\u0000add"

        /** How long "Undo" is offered after a swipe (the notice shows this long). */
        const val UNDO_MS = 6_000L
    }
}
