package com.ferrowl.app.push

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.ferrowl.app.MainActivity
import com.ferrowl.app.R
import com.ferrowl.core.Header
import com.ferrowl.core.InboxPeek
import com.ferrowl.core.translate
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/** Shows the "new mail" notifications (one per account, replaced at each update). */
@Singleton
class MailNotifier @Inject constructor(@ApplicationContext private val context: Context) {

    /**
     * Announces [account]'s new mail found by [peek] (unread above [last]):
     * by the push and by the periodic check alike, which share the memory of
     * what was announced, so each message is announced once. [quiet]: the
     * messages a rule keeps silent.
     *
     * - First time for the account (no memory yet): only the newest message,
     *   not every old unread one.
     * - The account's mailbox on screen: its list shows the message at once,
     *   so no pop-up over it, only the sound (as iOS does).
     * - A push with nothing new (read or seen in the app meanwhile): the
     *   newest unread message quietly, if no notification is up, so a push
     *   sent as urgent always shows something.
     */
    fun announce(account: String, peek: InboxPeek, last: Long, quiet: Set<UInt>, fromPush: Boolean) {
        if (peek.fresh.isEmpty()) {
            val newest = peek.newest
            if (fromPush && newest != null && !newest.seen && shownUids(account) == null) {
                show(listOf(newest), 1, account, listOf(newest.uid.toLong()), quietly = true)
            }
            return
        }
        val loud = peek.fresh.filter { it.uid !in quiet }
        val first = last == 0L
        val listed = if (first) listOf(loud.firstOrNull() ?: peek.fresh.first()) else loud.ifEmpty { peek.fresh }
        val count = if (first) 1 else peek.freshCount.toInt()
        // Enough to take the notification back as mail is read, never a huge list.
        val uids = if (first) listOf(listed.first().uid.toLong()) else peek.unreadUids.take(MAX_UIDS).map { it.toLong() }
        val silent = loud.isEmpty()
        val onScreen = ForegroundMailbox.shows(account)
        show(listed, count, account, uids, quietly = silent || onScreen)
        if (onScreen && !silent) playSound()
    }

    /** The notification sound, without a notification popping up (the mailbox on screen). */
    private fun playSound() {
        val manager = context.getSystemService(NotificationManager::class.java)
        val audio = context.getSystemService(android.media.AudioManager::class.java)
        // Do Not Disturb and silent/vibrate modes are respected.
        if (manager.currentInterruptionFilter != NotificationManager.INTERRUPTION_FILTER_ALL) return
        if (audio.ringerMode != android.media.AudioManager.RINGER_MODE_NORMAL) return
        val sound = manager.getNotificationChannel(CHANNEL)?.sound
            ?: android.media.RingtoneManager.getDefaultUri(android.media.RingtoneManager.TYPE_NOTIFICATION)
        runCatching { android.media.RingtoneManager.getRingtone(context, sound)?.play() }
    }

    /**
     * One message: sender and subject; several ([count] in all, the newest
     * of them in [fresh]): how many, and the newest. [uids]: every message
     * the notification is about, so it can be taken back once they are
     * read elsewhere. [quietly]: an update, without sound.
     */
    fun show(fresh: List<Header>, count: Int, account: String, uids: List<Long>, quietly: Boolean = false) {
        val newest = fresh.first()
        // The account, when there are several; tapping opens the newest message.
        val builder = base(account, newest.uid.toLong())
            .setSubText(account)
            .setOnlyAlertOnce(quietly)
            .setSilent(quietly)
            .addExtras(Bundle().apply { putLongArray(EXTRA_UIDS, uids.toLongArray()) })
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

    /**
     * The messages [account]'s notification on screen is about: null with
     * no notification, empty for a plain "New mail".
     */
    fun shownUids(account: String): List<Long>? {
        val manager = context.getSystemService(NotificationManager::class.java)
        val shown = manager.activeNotifications.firstOrNull { it.tag == account.lowercase() && it.id == ID } ?: return null
        return shown.notification.extras.getLongArray(EXTRA_UIDS)?.toList().orEmpty()
    }

    /** Takes back [account]'s notification. */
    fun cancel(account: String) = NotificationManagerCompat.from(context).cancel(account.lowercase(), ID)

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
        /** Message UIDs kept in a notification (to take it back as they are read). */
        private const val MAX_UIDS = 200
        private const val EXTRA_UIDS = "com.ferrowl.app.uids"
        private const val CHANNEL = "new_mail"
        private const val ID = 1
    }
}
