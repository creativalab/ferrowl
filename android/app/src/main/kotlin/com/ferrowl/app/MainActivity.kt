package com.ferrowl.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import com.ferrowl.app.data.Billing
import com.ferrowl.app.data.MicrosoftSignIn
import com.ferrowl.app.ui.navigation.FerrowlNavHost
import com.ferrowl.app.ui.notice.LocalBilling
import com.ferrowl.app.ui.notice.LocalNotices
import com.ferrowl.app.ui.notice.Notices
import com.ferrowl.app.ui.notice.NoticeHost
import com.ferrowl.app.ui.theme.FerrowlTheme
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

    @Inject lateinit var notificationOpen: com.ferrowl.app.push.NotificationOpen

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) handle(intent)
        setContent {
            FerrowlTheme {
                CompositionLocalProvider(LocalNotices provides notices, LocalBilling provides billing) {
                    // Notices (confirmations and errors) over every screen, at the bottom.
                    Box(Modifier.fillMaxSize()) {
                        FerrowlNavHost()
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

    private fun handle(intent: Intent?) {
        // A tapped new-mail notification: open its message.
        intent?.getStringExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_EMAIL)?.let { email ->
            val uid = intent.getLongExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_UID, -1).takeIf { it >= 0 }
            notificationOpen.request(email, uid)
            intent.removeExtra(com.ferrowl.app.push.NotificationOpen.EXTRA_EMAIL)
        }
        val uri = intent?.data ?: return
        if (microsoft.isAnswer(uri)) {
            microsoft.complete(uri)
            setIntent(Intent(intent).setData(null))
        }
    }
}
