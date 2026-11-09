package com.ferrowl.app.push

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.ferrowl.app.MainActivity
import com.ferrowl.app.R
import com.ferrowl.core.Header
import com.ferrowl.core.translate
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/** Shows the "new mail" notifications (one per account, replaced at each update). */
@Singleton
class MailNotifier @Inject constructor(@ApplicationContext private val context: Context) {

    /**
     * One message: sender and subject; several ([count] in all, the newest
     * of them in [fresh]): how many, and the newest.
     */
    fun show(fresh: List<Header>, count: Int, account: String) {
        val newest = fresh.first()
        // The account, when there are several; tapping opens the newest message.
        val builder = base(account, newest.uid.toLong()).setSubText(account)
        if (count <= 1) {
            builder.setContentTitle(newest.from).setContentText(newest.subject)
        } else {
            val style = NotificationCompat.InboxStyle()
            fresh.take(MAX_LINES).forEach { style.addLine("${it.from}: ${it.subject}") }
            builder
                .setContentTitle(translate("notif_many").replace("{n}", count.toString()))
                .setContentText("${newest.from}: ${newest.subject}")
                .setStyle(style)
                .setNumber(count)
        }
        post(builder, account)
    }

    /** When the inbox could not be read: just "New mail". */
    fun showGeneric(account: String?) =
        post(base(account, null).setContentTitle(translate("notif_new_mail")).also { b -> account?.let(b::setSubText) }, account)

    private fun base(account: String?, uid: Long?): NotificationCompat.Builder {
        ensureChannel()
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        if (account != null) {
            intent.putExtra(NotificationOpen.EXTRA_EMAIL, account)
            if (uid != null) intent.putExtra(NotificationOpen.EXTRA_UID, uid)
        }
        val open = PendingIntent.getActivity(
            context,
            // One per account, so each notification opens its own message.
            account?.lowercase()?.hashCode() ?: 0,
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setCategory(NotificationCompat.CATEGORY_EMAIL)
            .setContentIntent(open)
            .setAutoCancel(true)
    }

    /** One notification per account (replaced at each update). */
    private fun post(builder: NotificationCompat.Builder, account: String?) {
        // Android 13+: nothing to show without the user's permission.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        NotificationManagerCompat.from(context).notify(account?.lowercase(), ID, builder.build())
    }

    private fun ensureChannel() {
        val manager = context.getSystemService(NotificationManager::class.java)
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, translate("notif_channel"), NotificationManager.IMPORTANCE_DEFAULT),
            )
        }
    }

    companion object {
        /** Messages listed in a notification of several. */
        const val MAX_LINES = 5
        private const val CHANNEL = "new_mail"
        private const val ID = 1
    }
}
