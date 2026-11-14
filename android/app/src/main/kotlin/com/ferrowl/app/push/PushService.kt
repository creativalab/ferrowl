package com.ferrowl.app.push

import com.ferrowl.app.data.MailRepository
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import dagger.hilt.android.AndroidEntryPoint
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

    @Inject lateinit var mailCheck: MailCheck

    // Called on a background thread: the call to the server finishes here
    // (a coroutine of the service would be cut off when it is destroyed,
    // leaving the server with the old token).
    override fun onNewToken(token: String) {
        runBlocking { withTimeoutOrNull(TIMEOUT_MS) { registrar.tokenChanged(token) } }
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
            // An id this phone no longer knows (an account removed while
            // offline): not the account in use, whose mail it is not.
            if (email == null) {
                notifier.showGeneric(null)
                return@runBlocking
            }
            val shown = withTimeoutOrNull(TIMEOUT_MS) { notifyNewMessages(email) } ?: false
            if (!shown) notifier.showGeneric(email)
        }
    }

    /** Mail read elsewhere: notifications taken back (shared with the periodic check). */
    private suspend fun takeBackRead(email: String) = runCatching {
        mailCheck.takeBackRead(email) { after, limit -> repo.peekInbox(email, after, limit) }
    }

    /** True if it decided what to show (possibly nothing: already read). */
    private suspend fun notifyNewMessages(address: String): Boolean = runCatching {
        val last = store.current().lastNotified[address.lowercase()] ?: 0
        // Only what the notification shows: loading the inbox may not fit
        // in the time Android gives.
        val peek = repo.peekInbox(address, last, MailNotifier.MAX_LINES) ?: return@runCatching false
        accountUnread.set(address, peek.unread.toInt(), fresh = peek.fresh)
        // The user's rules may keep messages quiet: then no sound.
        val quiet = if (peek.fresh.isEmpty()) emptySet() else runCatching { repo.silenced(address, peek.fresh) }.getOrDefault(emptySet())
        notifier.announce(address, peek, last, quiet, fromPush = true)
        if (peek.fresh.isNotEmpty()) {
            // Connected automation apps (Tasker…) hear about it too.
            runCatching { bridge.newMail(address, peek) }
            store.setLastNotified(address, peek.fresh.first().uid.toLong())
        }
        true
    }.getOrDefault(false)

    private companion object {
        /** Well inside the time Android gives (cold start included). */
        const val TIMEOUT_MS = 10_000L
    }
}
