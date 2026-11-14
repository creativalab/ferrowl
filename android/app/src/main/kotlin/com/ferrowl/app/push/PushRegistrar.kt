package com.ferrowl.app.push

import android.util.Base64
import com.ferrowl.app.data.AccountStore
import com.ferrowl.app.data.Billing
import com.ferrowl.core.Account
import com.ferrowl.core.Protocol
import com.ferrowl.core.pushAccountJson
import com.ferrowl.core.translate
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.tasks.await
import java.security.SecureRandom
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Keeps this phone registered on the push server while instant
 * notifications are on:
 *
 * 1. the phone registers with its Firebase token and gets a secret device
 *    token;
 * 2. it creates a random 256-bit key, kept only here (encrypted with the
 *    Android Keystore), and sends every account with it (IMAP with a
 *    password; after the first one only with Ferrowl Pro): the server
 *    stores them sealed with that key and watches their inboxes;
 * 3. at every app start it sends the key again (`unlock`), because the
 *    server keeps it in memory only and forgets it when it restarts.
 */
@Singleton
class PushRegistrar @Inject constructor(
    private val store: PushStore,
    private val client: PushClient,
    private val accounts: AccountStore,
    private val billing: Billing,
) {
    private val lock = Mutex()

    /** Whether the first-launch question is still to be asked. */
    suspend fun needsAsking(): Boolean = !store.current().asked

    /** Turns notifications on. Throws with a message to show if it fails. */
    suspend fun enable() {
        val all = accounts.accounts.first()
        val first = all.firstOrNull() ?: return
        if (watched(all).isEmpty()) {
            if (first.protocol == Protocol.POP3) throw PushException(translate("push_pop3"), 0)
            throw PushException(translate("push_oauth"), 0)
        }
        store.setEnabled(true)
        try {
            sync()
        } catch (e: Exception) {
            store.setEnabled(false)
            throw e
        }
    }

    /** Turns notifications off and removes everything from the server. */
    suspend fun disable() = lock.withLock {
        val s = store.current()
        store.setEnabled(false)
        s.deviceToken?.let { runCatching { client.forget(it) } }
        store.clearRegistration()
    }

    /**
     * The accounts the server can watch: IMAP with a password (signed-in
     * Microsoft/Google accounts are not supported there yet), and after
     * the first one only with Ferrowl Pro.
     */
    private fun watched(all: List<Account>): List<Account> =
        all.filterIndexed { i, a -> (i == 0 || billing.pro.value) && a.protocol != Protocol.POP3 && a.oauth == null }

    /** Brings the registration up to date (app start, accounts changed). */
    suspend fun sync(): Unit = lock.withLock {
        // Nothing goes to the server before the user said yes.
        if (!store.current().enabled || !store.current().asked) return
        val wanted = watched(accounts.accounts.first())
        val fcmToken = FirebaseMessaging.getInstance().token.await()
        var s = store.current()
        if (s.deviceToken != null) {
            try {
                client.updateToken(s.deviceToken!!, fcmToken)
            } catch (e: PushException) {
                // The server no longer knows this phone: register again.
                if (e.status != 401) throw e
                store.clearRegistration()
                s = store.current()
            }
        }
        val token = s.deviceToken ?: client.register(fcmToken).device_token.also { newToken ->
            store.saveDevice(newToken, newKey())
        }
        val key = store.current().key ?: return
        // An older version watched one account without saying which: start over.
        store.legacyAccount()?.let { old ->
            runCatching { client.deleteAccount(token, old) }
            store.clearLegacyAccount()
        }
        // Removals that failed earlier (offline).
        for (id in store.current().removing) {
            if (remove(token, id)) store.setRemoving(id, add = false)
        }
        val known = store.current().accounts
        // The server restarted meanwhile: give it the key again first.
        if (known.isNotEmpty()) client.unlock(token, key)
        for ((email, id) in known) {
            if (wanted.none { it.email.equals(email, ignoreCase = true) }) {
                // Kept for later if the server cannot be reached: it would
                // otherwise go on watching the account with its password.
                if (!remove(token, id)) store.setRemoving(id, add = true)
                store.removeAccount(email)
            }
        }
        for (account in wanted) {
            if (known.keys.none { it.equals(account.email, ignoreCase = true) }) {
                store.saveAccount(account.email, client.addAccount(token, key, pushAccountJson(account)))
            }
        }
    }

    /** The accounts changed (added, removed, Pro bought or ended): watch the right ones. */
    suspend fun accountChanged() = sync()

    /** An account was set up again (maybe a new password): the server gets its new settings. */
    suspend fun accountSaved(email: String) {
        lock.withLock {
            val s = store.current()
            val id = s.accounts[email.lowercase()]
            if (s.deviceToken != null && id != null) {
                if (!remove(s.deviceToken, id)) store.setRemoving(id, add = true)
                store.removeAccount(email)
            }
        }
        sync()
    }

    /** Removes an account id from the server; false when it could not be reached. */
    private suspend fun remove(token: String, id: String): Boolean = try {
        client.deleteAccount(token, id)
        true
    } catch (e: PushException) {
        // Already gone (or the phone with it): nothing left to remove.
        e.status == 404 || e.status == 401
    } catch (e: Exception) {
        false
    }

    /** Mail of [email] up to [uid] was seen in the app: a push does not announce it again. */
    suspend fun seen(email: String, uid: Long) = store.setLastNotified(email, uid)

    /** Firebase gave the phone a new token. */
    suspend fun tokenChanged(fcmToken: String) {
        val s = store.current()
        if (s.enabled && s.deviceToken != null) runCatching { client.updateToken(s.deviceToken, fcmToken) }
    }

    private fun newKey(): String {
        val bytes = ByteArray(32).also { SecureRandom().nextBytes(it) }
        return Base64.encodeToString(bytes, Base64.NO_WRAP)
    }
}
