package com.ferrowl.app

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.core.content.IntentCompat
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MicrosoftSignIn
import com.ferrowl.app.data.SettingsStore
import com.ferrowl.app.push.ComposeRequest
import com.ferrowl.app.push.ComposeRequests
import com.ferrowl.app.ui.navigation.FerrowlNavHost
import com.ferrowl.app.ui.notice.LocalBilling
import com.ferrowl.app.ui.notice.LocalNotices
import com.ferrowl.app.ui.notice.NoticeHost
import com.ferrowl.app.ui.notice.Notices
import com.ferrowl.app.ui.theme.FerrowlTheme
import com.ferrowl.app.ui.theme.LocalRemoteImages
import com.ferrowl.core.FerrowlLink
import com.ferrowl.core.parseLink
import dagger.hilt.android.AndroidEntryPoint
import javax.inject.Inject

/**
 * The only visible Activity: every screen is a Compose destination inside
 * [FerrowlNavHost] (single-Activity architecture). It also receives the
 * answer of a Microsoft sign-in, forwarded by [OAuthRedirectActivity].
 */
@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    @Inject lateinit var microsoft: MicrosoftSignIn

    @Inject lateinit var notices: Notices

    @Inject lateinit var billing: Billing

    @Inject lateinit var settings: SettingsStore

    @Inject lateinit var notificationOpen: com.ferrowl.app.push.NotificationOpen

    @Inject lateinit var composeRequests: ComposeRequests

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) handle(intent)
        setContent {
            val theme by settings.theme.collectAsState(initial = "auto")
            val remoteImages by settings.remoteImages.collectAsState(initial = true)
            FerrowlTheme(theme) {
                CompositionLocalProvider(
                    LocalNotices provides notices,
                    LocalBilling provides billing,
                    LocalRemoteImages provides remoteImages,
                ) {
                    // Notices (confirmations and errors) over every screen, at the bottom.
                    Box(Modifier.fillMaxSize()) {
                        FerrowlNavHost(composeRequests)
                        NoticeHost(notices, Modifier.align(Alignment.BottomCenter))
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Ferrowl Pro: ask Google Play each time (bought or cancelled elsewhere meanwhile).
        billing.start()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    /** The files of a "share" intent (one or several). */
    private fun sharedFiles(intent: Intent): List<Uri> {
        val streams = if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
            IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
        } else {
            listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
        }
        // Some apps only put them in the clip data.
        val clips = intent.clipData?.let { c -> (0 until c.itemCount).mapNotNull { c.getItemAt(it).uri } }.orEmpty()
        return (streams + clips).distinct().filter(::isForeignContent)
    }

    /**
     * Only files another app hands over (content:// of its own provider).
     * A file:// address, or one of Ferrowl's own provider, would be read
     * with Ferrowl's rights: another app could have the mail cache attached
     * to a message to an address it chose.
     */
    private fun isForeignContent(uri: Uri): Boolean =
        uri.scheme.equals("content", ignoreCase = true) && uri.authority != "$packageName.files"

    private fun handle(intent: Intent?) {
        // Opened again from Recents: the intent is the old one (a notification
        // already opened, a share already turned into a message).
        if (intent != null && intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        // A tapped new-mail notification: open its message.
        intent?.getStringExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_EMAIL)?.let { email ->
            val uid = intent.getLongExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_UID, -1).takeIf { it >= 0 }
            notificationOpen.request(email, uid)
            intent.removeExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_EMAIL)
        }
        if (intent == null) return
        // Text or files shared from another app: a new message with them.
        if (intent.action == Intent.ACTION_SEND || intent.action == Intent.ACTION_SEND_MULTIPLE) {
            composeRequests.request(
                ComposeRequest(
                    to = intent.getStringArrayExtra(Intent.EXTRA_EMAIL)?.joinToString(", ").orEmpty(),
                    subject = intent.getStringExtra(Intent.EXTRA_SUBJECT).orEmpty(),
                    body = intent.getStringExtra(Intent.EXTRA_TEXT).orEmpty(),
                    files = sharedFiles(intent).map(Uri::toString),
                ),
            )
            setIntent(Intent(intent).setAction(Intent.ACTION_MAIN))
            return
        }
        val uri = intent.data ?: return
        if (microsoft.isAnswer(uri)) {
            microsoft.complete(uri)
            setIntent(Intent(intent).setData(null))
            return
        }
        // ferrowl:// and mailto: links (read by the engine, as on iOS and desktop).
        when (val link = parseLink(uri.toString())) {
            is FerrowlLink.Open -> notificationOpen.request(link.account, link.uid.toLong())
            is FerrowlLink.Inbox -> link.account?.let { notificationOpen.request(it, null) }
            is FerrowlLink.Compose -> composeRequests.request(
                ComposeRequest(
                    to = link.to,
                    cc = link.cc,
                    // "Send e-mail" intents may carry these as extras too.
                    subject = link.subject.ifEmpty { intent.getStringExtra(Intent.EXTRA_SUBJECT).orEmpty() },
                    body = link.body.ifEmpty { intent.getStringExtra(Intent.EXTRA_TEXT).orEmpty() },
                ),
            )
            null -> return
        }
        setIntent(Intent(intent).setData(null))
    }
}
