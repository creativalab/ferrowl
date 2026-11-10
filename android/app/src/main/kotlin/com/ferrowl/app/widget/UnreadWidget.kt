package com.ferrowl.app.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.view.View
import android.widget.RemoteViews
import androidx.core.net.toUri
import com.ferrowl.app.MainActivity
import com.ferrowl.app.R
import com.ferrowl.app.data.AccountUnread

/**
 * The Home Screen widget: unread mail of every account, with the newest
 * unread messages (each opens its message). It shows what [AccountUnread]
 * keeps, and is redrawn whenever that changes, so it never polls.
 */
class UnreadWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        draw(context, manager, ids)
    }

    companion object {
        private data class Line(val line: Int, val row: Int, val from: Int, val subject: Int, val read: Int, val archive: Int)

        private val LINES = listOf(
            Line(R.id.widget_line1, R.id.widget_row1, R.id.widget_from1, R.id.widget_subject1, R.id.widget_read1, R.id.widget_archive1),
            Line(R.id.widget_line2, R.id.widget_row2, R.id.widget_from2, R.id.widget_subject2, R.id.widget_read2, R.id.widget_archive2),
            Line(R.id.widget_line3, R.id.widget_row3, R.id.widget_from3, R.id.widget_subject3, R.id.widget_read3, R.id.widget_archive3),
        )

        /** "Read" or "archive" for a message, run by [WidgetActionReceiver]. */
        private fun act(context: Context, link: String, archive: Boolean, code: Int): PendingIntent = PendingIntent.getBroadcast(
            context,
            code,
            Intent(context, WidgetActionReceiver::class.java)
                .putExtra(WidgetActionReceiver.EXTRA_LINK, link)
                .putExtra(WidgetActionReceiver.EXTRA_ARCHIVE, archive),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        /** Redraws every Ferrowl widget on the Home Screen (none: nothing to do). */
        fun refresh(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val ids = manager.getAppWidgetIds(ComponentName(context, UnreadWidget::class.java))
            if (ids.isNotEmpty()) draw(context, manager, ids)
        }

        private fun open(context: Context, link: String, code: Int): PendingIntent = PendingIntent.getActivity(
            context,
            code,
            Intent(Intent.ACTION_VIEW, link.toUri(), context, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

        private fun draw(context: Context, manager: AppWidgetManager, ids: IntArray) {
            val (total, recent) = AccountUnread.snapshot(context)
            val views = RemoteViews(context.packageName, R.layout.widget_unread)
            views.setTextViewText(R.id.widget_count, total.toString())
            views.setOnClickPendingIntent(R.id.widget_root, open(context, "ferrowl://inbox", 0))
            views.setViewVisibility(R.id.widget_none, if (recent.isEmpty()) View.VISIBLE else View.GONE)
            LINES.forEachIndexed { i, l ->
                val mail = recent.getOrNull(i)
                views.setViewVisibility(l.line, if (mail == null) View.GONE else View.VISIBLE)
                if (mail != null) {
                    views.setTextViewText(l.from, mail.from)
                    views.setTextViewText(l.subject, mail.subject)
                    views.setOnClickPendingIntent(l.row, open(context, mail.link, i + 1))
                    views.setOnClickPendingIntent(l.read, act(context, mail.link, archive = false, code = 10 + i))
                    views.setOnClickPendingIntent(l.archive, act(context, mail.link, archive = true, code = 20 + i))
                }
            }
            manager.updateAppWidget(ids, views)
        }
    }
}
