package com.ferrowl.app.push

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.ferrowl.app.bridge.Bridge
import com.ferrowl.app.data.AccountUnread
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MailRepository
import com.ferrowl.core.Engine
import com.ferrowl.core.InboxPeek
import dagger.hilt.EntryPoint
import dagger.hilt.InstallIn
import dagger.hilt.android.EntryPointAccessors
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.util.Locale
import java.util.concurrent.TimeUnit
import javax.inject.Inject
import javax.inject.Singleton

/**
 * New mail without the push server (POP3, signed-in Google/Microsoft
 * accounts, instant notifications off, or a push lost on the way), as iOS
 * does: Android runs [MailCheckWorker] about every 15 minutes, and while the
 * app is open it checks every 3 minutes. Every account that may be used
 * (the first one, all with Ferrowl Pro) is read on a connection of its own;
 * new unread mail is announced with the same rules and the same memory as a
 * push (each message once), notifications of mail read elsewhere are taken
 * back, and the counts and the widget are updated.
 */
@Singleton
class MailCheck @Inject constructor(
    @ApplicationContext private val context: Context,
    private val repo: MailRepository,
    private val billing: Billing,
    private val store: PushStore,
    private val notifier: MailNotifier,
    private val accountUnread: AccountUnread,
    private val bridge: Bridge,
) {
    /** The background task and the open app's timer never check at once. */
    private val lock = Mutex()

    /**
     * Checks every account that may be used; false when a server could not
     * be reached. With the app open ([foreground]) the account in use is not
     * announced: its list is fetched again instead (it is on screen).
     */
    suspend fun checkAll(foreground: Boolean): Boolean {
        if (lock.isLocked) return true
        return lock.withLock { withContext(Dispatchers.IO) { checkLocked(foreground) } }
    }

    private suspend fun checkLocked(foreground: Boolean): Boolean {
        val all = repo.accountList.first()
        val active = repo.savedAccount()?.email
        var ok = true
        all.forEachIndexed { i, account ->
            // Without Ferrowl Pro only the first account is used.
            if (i > 0 && !billing.pro.value) return@forEachIndexed
            if (foreground && account.email.equals(active, ignoreCase = true)) {
                repo.arrived(account.email)
                return@forEachIndexed
            }
            // An engine of its own, closed after use: the app's stays on its account.
            val checked = Engine(context.filesDir.resolve("mail").path, Locale.getDefault().language).use { engine ->
                withTimeoutOrNull(ACCOUNT_TIMEOUT_MS) {
                    runCatching { check(account.email) { after, limit -> engine.peekInbox(account, after.toUInt(), limit.toUInt()) } }
                        .getOrDefault(false)
                } ?: false
            }
            // POP3 and servers that cannot be read now: skipped (as iOS).
            if (!checked) ok = false
        }
        return ok
    }

    /** One account: announces its new mail, or takes back what was read. */
    private suspend fun check(email: String, peek: suspend (after: Long, limit: Int) -> InboxPeek): Boolean {
        val last = store.current().lastNotified[email.lowercase()] ?: 0
        val found = peek(last, MailNotifier.MAX_LINES)
        accountUnread.set(email, found.unread.toInt(), fresh = found.fresh)
        if (found.fresh.isEmpty()) {
            takeBackRead(email, peek)
            return true
        }
        val quiet = runCatching { repo.silenced(email, found.fresh) }.getOrDefault(emptySet())
        notifier.announce(email, found, last, quiet, fromPush = false)
        runCatching { bridge.newMail(email, found) }
        store.setLastNotified(email, found.fresh.first().uid.toLong())
        return true
    }

    /**
     * Mail of [email] was read (or removed) elsewhere: its notification goes
     * once none of its messages is unread, or is updated quietly to the
     * ones still unread. Used by the push ("read") and the periodic check.
     */
    suspend fun takeBackRead(email: String, peek: suspend (after: Long, limit: Int) -> InboxPeek?) {
        val shown = notifier.shownUids(email)
        if (shown == null) {
            // No notification on screen: only the counts and the widget.
            val found = peek(0, 0) ?: return
            accountUnread.set(email, found.unread.toInt())
            accountUnread.dropRead(email, found.unreadUids.map { it.toLong() }.toSet(), 0)
            return
        }
        if (shown.isEmpty()) {
            // A plain "New mail": it goes once nothing is unread.
            val found = peek(UInt.MAX_VALUE.toLong(), 0) ?: return
            accountUnread.set(email, found.unread.toInt())
            if (found.unread == 0u) notifier.cancel(email)
            return
        }
        val found = peek(shown.min() - 1, MailNotifier.MAX_LINES) ?: return
        accountUnread.set(email, found.unread.toInt())
        val unread = found.unreadUids.map { it.toLong() }.toSet()
        accountUnread.dropRead(email, unread, shown.min())
        val still = shown.filter { it in unread }
        val listed = found.fresh.filter { it.uid.toLong() in still }
        when {
            still.isEmpty() -> notifier.cancel(email)
            still.size < shown.size && listed.isNotEmpty() ->
                notifier.show(listed, still.size, email, still, quietly = true)
        }
    }

    /** Starts the periodic check once there are accounts, stops it when none is left. */
    fun schedule(hasAccounts: Boolean) {
        val work = WorkManager.getInstance(context)
        if (!hasAccounts) {
            work.cancelUniqueWork(WORK)
            return
        }
        val request = PeriodicWorkRequestBuilder<MailCheckWorker>(15, TimeUnit.MINUTES)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .build()
        work.enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.KEEP, request)
    }

    private companion object {
        const val WORK = "mail-check"
        /** One server that does not answer must not hold up the others. */
        const val ACCOUNT_TIMEOUT_MS = 60_000L
    }
}

/** How the worker (made by WorkManager, not by Hilt) reaches [MailCheck]. */
@EntryPoint
@InstallIn(SingletonComponent::class)
interface MailCheckEntryPoint {
    fun mailCheck(): MailCheck
}

/** The periodic background check (see [MailCheck]). */
class MailCheckWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val check = EntryPointAccessors.fromApplication(applicationContext, MailCheckEntryPoint::class.java).mailCheck()
        check.checkAll(foreground = false)
        // A server that could not be read is tried at the next run, not retried now.
        return Result.success()
    }
}
