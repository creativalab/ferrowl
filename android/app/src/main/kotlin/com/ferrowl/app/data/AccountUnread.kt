package com.ferrowl.app.data

import android.content.Context
import androidx.core.content.edit
import com.ferrowl.app.widget.UnreadWidget
import com.ferrowl.core.Header
import com.ferrowl.core.messageLink
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import javax.inject.Inject
import javax.inject.Singleton

/** A message the Home Screen widget shows. */
@Serializable
data class WidgetMail(val from: String, val subject: String, val date: Long, val uid: Long, val link: String)

/**
 * Each account's unread count in its inbox, shown next to it in the
 * drawer and on the Home Screen widget, with its newest unread messages
 * for the widget: written when the mailbox loads the account's inbox and
 * when a push checks an account (Gmail: the Primary tab, as notifications
 * count). Kept on the phone so the accounts not in use have a number too.
 */
@Singleton
class AccountUnread @Inject constructor(@ApplicationContext private val context: Context) {
    private val prefs = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
    private val _counts = MutableStateFlow(load())
    val counts: StateFlow<Map<String, Int>> = _counts.asStateFlow()

    private fun load(): Map<String, Int> =
        prefs.all.mapNotNull { (k, v) -> (v as? Int)?.let { k to it } }.toMap()

    /**
     * An account's count; `recent` (its inbox's messages) replaces the
     * widget's newest unread ones, `fresh` (just notified) goes in front of
     * them. With neither, they stay (all go when nothing is unread).
     */
    @Synchronized
    fun set(email: String, unread: Int, recent: List<Header>? = null, fresh: List<Header>? = null) {
        val key = email.lowercase()
        val mails = { headers: List<Header> ->
            headers.filter { !it.seen }.map {
                WidgetMail(
                    from = it.from.ifBlank { it.fromAddress },
                    subject = it.subject,
                    date = it.timestamp,
                    uid = it.uid.toLong(),
                    link = messageLink(email, "INBOX", it.uid),
                )
            }
        }
        val old = recentOf(key)
        val next = when {
            recent != null -> mails(recent)
            fresh != null -> mails(fresh).let { f -> f + old.filter { o -> f.none { it.uid == o.uid } } }
            unread == 0 -> emptyList()
            else -> old
        }.sortedByDescending { it.date }.take(RECENT)
        if (_counts.value[key] == unread && next == old) return
        prefs.edit {
            putInt(key, unread)
            putString(RECENT_PREFIX + key, json.encodeToString(next))
        }
        _counts.value = _counts.value + (key to unread)
        UnreadWidget.refresh(context)
    }

    /** Mail read elsewhere: of the messages from UID `from` on, only the still unread stay. */
    @Synchronized
    fun dropRead(email: String, unread: Set<Long>, from: Long) {
        val key = email.lowercase()
        val old = recentOf(key)
        val next = old.filterNot { it.uid >= from && it.uid !in unread }
        if (next == old) return
        prefs.edit { putString(RECENT_PREFIX + key, json.encodeToString(next)) }
        UnreadWidget.refresh(context)
    }

    /** What an account had before a widget button changed it. */
    data class Before(val key: String, val unread: Int, val recent: List<WidgetMail>)

    /**
     * A message read or archived from the widget: it leaves at once. Returns
     * what the account had, to [restore] if the server refuses.
     */
    @Synchronized
    fun removeMail(link: String): Before? {
        val owner = prefs.all.keys.filter { it.startsWith(RECENT_PREFIX) }
            .firstOrNull { k -> recentOf(k.removePrefix(RECENT_PREFIX)).any { it.link == link } }
            ?.removePrefix(RECENT_PREFIX) ?: return null
        val before = Before(owner, _counts.value[owner] ?: 0, recentOf(owner))
        val next = recentOf(owner).filterNot { it.link == link }
        val unread = (_counts.value[owner] ?: 0) - 1
        prefs.edit {
            putString(RECENT_PREFIX + owner, json.encodeToString(next))
            putInt(owner, unread.coerceAtLeast(0))
        }
        _counts.value = _counts.value + (owner to unread.coerceAtLeast(0))
        UnreadWidget.refresh(context)
        return before
    }

    /** Puts back what [removeMail] changed (the widget button's action failed). */
    @Synchronized
    fun restore(before: Before) {
        prefs.edit {
            putInt(before.key, before.unread)
            putString(RECENT_PREFIX + before.key, json.encodeToString(before.recent))
        }
        _counts.value = _counts.value + (before.key to before.unread)
        UnreadWidget.refresh(context)
    }

    /** A removed account: its numbers go. */
    @Synchronized
    fun forget(email: String) {
        val key = email.lowercase()
        prefs.edit {
            remove(key)
            remove(RECENT_PREFIX + key)
        }
        _counts.value = _counts.value - key
        UnreadWidget.refresh(context)
    }

    fun of(email: String): Int = _counts.value[email.lowercase()] ?: 0

    private fun recentOf(key: String): List<WidgetMail> = read(prefs.getString(RECENT_PREFIX + key, null))

    companion object {
        private const val FILE = "account_unread"
        private const val RECENT_PREFIX = "recent:"
        private const val RECENT = 5
        private val json = Json { ignoreUnknownKeys = true }

        private fun read(text: String?): List<WidgetMail> =
            text?.let { runCatching { json.decodeFromString<List<WidgetMail>>(it) }.getOrNull() }.orEmpty()

        /** What the widget shows, read without the app's objects: the total and the newest unread. */
        fun snapshot(context: Context): Pair<Int, List<WidgetMail>> {
            val all = context.getSharedPreferences(FILE, Context.MODE_PRIVATE).all
            val total = all.values.sumOf { (it as? Int) ?: 0 }
            val recent = all.filterKeys { it.startsWith(RECENT_PREFIX) }.values
                .flatMap { read(it as? String) }
                .sortedByDescending { it.date }
                .take(RECENT)
            return total to recent
        }
    }
}
