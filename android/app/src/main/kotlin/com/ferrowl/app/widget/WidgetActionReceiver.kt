package com.ferrowl.app.widget

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.ferrowl.app.data.AccountUnread
import com.ferrowl.app.data.MailRepository
import com.ferrowl.core.Engine
import com.ferrowl.core.FerrowlLink
import com.ferrowl.core.parseLink
import dagger.hilt.android.AndroidEntryPoint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.util.Locale
import javax.inject.Inject

/** Work that outlives the receiver's onReceive. */
private val widgetScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

/**
 * The widget's buttons on a message: mark it read or archive it, without
 * opening the app (not exported: only Ferrowl's widget sends these). The
 * message leaves the widget at once; the server follows.
 */
@AndroidEntryPoint
class WidgetActionReceiver : BroadcastReceiver() {
    @Inject lateinit var repo: MailRepository

    @Inject lateinit var accountUnread: AccountUnread

    override fun onReceive(context: Context, intent: Intent) {
        val link = intent.getStringExtra(EXTRA_LINK) ?: return
        val archive = intent.getBooleanExtra(EXTRA_ARCHIVE, false)
        val open = parseLink(link) as? FerrowlLink.Open ?: return
        accountUnread.removeMail(link)
        val done = goAsync()
        widgetScope.launch {
            try {
                withTimeoutOrNull(40_000L) {
                    val account = repo.accountList.first().firstOrNull { it.email.equals(open.account, ignoreCase = true) }
                        ?: return@withTimeoutOrNull
                    // An engine of its own: the app's may be busy, or on another account.
                    val engine = Engine(context.filesDir.resolve("mail").path, Locale.getDefault().language)
                    runCatching {
                        engine.connect(account)
                        if (archive) engine.archiveMany(open.folder, listOf(open.uid)) else engine.setSeenMany(open.folder, listOf(open.uid), true)
                    }
                    repo.arrived(account.email)
                }
            } finally {
                done.finish()
            }
        }
    }

    companion object {
        const val EXTRA_LINK = "link"
        const val EXTRA_ARCHIVE = "archive"
    }
}
