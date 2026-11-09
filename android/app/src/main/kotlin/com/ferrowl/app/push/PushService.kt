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
 * Receives the server's "new mail" signal (a data-only Firebase message, so
 * the app decides what to show). It reads the inbox and notifies the new
 * unread messages; if that fails in time, a plain "New mail" is shown.
 */
@AndroidEntryPoint
class PushService : FirebaseMessagingService() {
    @Inject lateinit var repo: MailRepository

    @Inject lateinit var store: PushStore

    @Inject lateinit var registrar: PushRegistrar

    @Inject lateinit var notifier: MailNotifier

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onNewToken(token: String) {
        scope.launch { registrar.tokenChanged(token) }
    }

    // Called on a background thread; Android gives it about 20 seconds.
    override fun onMessageReceived(message: RemoteMessage) {
        if (message.data["type"] != "new_mail") return
        runBlocking {
            // Which account has new mail (the server sends its id).
            val email = message.data["account"]?.let { store.current().emailOf(it) }
            val shown = withTimeoutOrNull(TIMEOUT_MS) { notifyNewMessages(email) } ?: false
            if (!shown) notifier.showGeneric(email)
        }
    }

    /** True if it decided what to show (possibly nothing: already read). */
    private suspend fun notifyNewMessages(email: String?): Boolean = runCatching {
        val address = email ?: repo.savedAccount()?.email ?: return@runCatching false
        val last = store.current().lastNotified[address.lowercase()] ?: 0
        // Only what the notification shows: loading the inbox may not fit
        // in the time Android gives.
        val peek = repo.peekInbox(address, last, MailNotifier.MAX_LINES) ?: return@runCatching false
        if (peek.fresh.isNotEmpty()) {
            notifier.show(peek.fresh, peek.freshCount.toInt(), address)
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
