package com.ferrowl.app.data

import android.content.Context
import androidx.core.content.edit
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Each account's unread count in its inbox, shown next to it in the
 * drawer: written when the mailbox loads the account's inbox and when a
 * push checks an account (Gmail: the Primary tab, as notifications count).
 * Kept on the phone so the accounts not in use have a number too.
 */
@Singleton
class AccountUnread @Inject constructor(@ApplicationContext context: Context) {
    private val prefs = context.getSharedPreferences("account_unread", Context.MODE_PRIVATE)
    private val _counts = MutableStateFlow(load())
    val counts: StateFlow<Map<String, Int>> = _counts.asStateFlow()

    private fun load(): Map<String, Int> =
        prefs.all.mapNotNull { (k, v) -> (v as? Int)?.let { k to it } }.toMap()

    fun set(email: String, unread: Int) {
        val key = email.lowercase()
        if (_counts.value[key] == unread) return
        prefs.edit { putInt(key, unread) }
        _counts.value = _counts.value + (key to unread)
    }

    fun of(email: String): Int = _counts.value[email.lowercase()] ?: 0
}
