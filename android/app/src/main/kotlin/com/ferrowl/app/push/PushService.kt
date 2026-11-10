package com.ferrowl.app.push

import com.ferrowl.app.data.MailRepository
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import dagger.hilt.android.AndroidEntryPoint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/**
 * Receives the server's signals (data-only Firebase messages, so the app
 * decides what to show). "New mail": it reads the inbox and notifies the
 * new unread messages; if that fails in time, a plain "New mail" is shown.
 * "Read": mail was read elsewhere, and notifications of messages no longer
 * unread are taken back.
 */
@AndroidEntryPoint
class PushService : FirebaseMessagingService() {
    @Inject lateinit var repo: MailRepository

    @Inject lateinit var store: PushStore

    @Inject lateinit var registrar: PushRegistrar

    @Inject lateinit var notifier: MailNotifier

    @Inject lateinit var bridge: com.ferrowl.app.bridge.Bridge

    @Inject lateinit var accountUnread: com.ferrowl.app.data.AccountUnread

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onNewToken(token: String) {
        scope.launch { registrar.tokenChanged(token) }
    }

    // Called on a background thread; Android gives it about 20 seconds.
    override fun onMessageReceived(message: RemoteMessage) {
        val type = message.data["type"]
        if (type != "new_mail" && type != "read") return
        runBlocking {
            // Which account (the server sends its id).
            val email = message.data["account"]?.let { store.current().emailOf(it) }
            // An open mailbox of this account shows the change at once.
            email?.let { repo.arrived(it) }
            if (type == "read") {
                email?.let { withTimeoutOrNull(TIMEOUT_MS) { takeBackRead(it) } }
                return@runBlocking
            }
            val shown = withTimeoutOrNull(TIMEOUT_MS) { notifyNewMessages(email) } ?: false
            if (!shown) notifier.showGeneric(email)
        }
    }

    /**
     * Mail of [email] was read (or removed) elsewhere: its notification goes
     * once none of its messages is unread, or is updated quietly to the
     * ones still unread.
     */
    private suspend fun takeBackRead(email: String) = runCatching {
        val shown = notifier.shownUids(email)
        if (shown == null) {
            // No notification on screen: only the counts and the widget.
            val peek = repo.peekInbox(email, 0, 0) ?: return@runCatching
            accountUnread.set(email, peek.unread.toInt())
            accountUnread.dropRead(email, peek.unreadUids.map { it.toLong() }.toSet(), 0)
            return@runCatching
        }
        if (shown.isEmpty()) {
            // A plain "New mail": it goes once nothing is unread.
            val peek = repo.peekInbox(email, UInt.MAX_VALUE.toLong(), 0) ?: return@runCatching
            accountUnread.set(email, peek.unread.toInt())
            if (peek.unread == 0u) notifier.cancel(email)
            return@runCatching
        }
        val peek = repo.peekInbox(email, shown.min() - 1, MailNotifier.MAX_LINES) ?: return@runCatching
        accountUnread.set(email, peek.unread.toInt())
        val unread = peek.unreadUids.map { it.toLong() }.toSet()
        accountUnread.dropRead(email, unread, shown.min())
        val still = shown.filter { it in unread }
        val listed = peek.fresh.filter { it.uid.toLong() in still }
        when {
            still.isEmpty() -> notifier.cancel(email)
            still.size < shown.size && listed.isNotEmpty() ->
                notifier.show(listed, still.size, email, still, quietly = true)
        }
    }

    /** True if it decided what to show (possibly nothing: already read). */
    private suspend fun notifyNewMessages(email: String?): Boolean = runCatching {
        val address = email ?: repo.savedAccount()?.email ?: return@runCatching false
        val last = store.current().lastNotified[address.lowercase()] ?: 0
        // Only what the notification shows: loading the inbox may not fit
        // in the time Android gives.
        val peek = repo.peekInbox(address, last, MailNotifier.MAX_LINES) ?: return@runCatching false
        accountUnread.set(address, peek.unread.toInt(), fresh = peek.fresh)
        if (peek.fresh.isNotEmpty()) {
            notifier.show(peek.fresh, peek.freshCount.toInt(), address, peek.unreadUids.map { it.toLong() })
            // Connected automation apps (Tasker…) hear about it too.
            runCatching { bridge.newMail(address, peek) }
            store.setLastNotified(address, peek.fresh.first().uid.toLong())
        }
        true
    }.getOrDefault(false)

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    private companion object {
        const val TIMEOUT_MS = 15_000L
    }
}
