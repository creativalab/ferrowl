package com.ferrowl.app.bridge

import android.content.Context
import androidx.core.content.edit
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.security.SecureRandom
import javax.inject.Inject
import javax.inject.Singleton

/**
 * An app the user allowed to use Ferrowl (Tasker, MacroDroid, Automate…).
 * It proves who it is with [token] (the key the user pasted into it), and
 * Ferrowl's answers and events go only to [packageName], so no other app
 * can read them.
 */
@Serializable
data class Connection(
    val token: String,
    val packageName: String,
    val label: String,
    /** May list unread mail, search, and receive new-mail events. */
    val read: Boolean = true,
    /** May send mail (Ferrowl Pro). */
    val send: Boolean = false,
    /** May change mail: archive, mark read or unread (Ferrowl Pro). */
    val write: Boolean = false,
    /** Each message to send, and each change, is confirmed by the user in a notification first. */
    val askBeforeSend: Boolean = true,
    val lastUsed: Long = 0,
)

/** One line of the activity log shown in the settings. */
@Serializable
data class LogEntry(val time: Long, val label: String, val action: String, val outcome: String)

/** A message an app asked to send, or a change to mail, waiting for the user's answer. */
@Serializable
data class PendingSend(
    val id: String,
    val token: String,
    val request: String,
    val account: String,
    val to: String = "",
    val subject: String = "",
    val body: String = "",
    /** The request's action: SEND, ARCHIVE, MARK_READ or MARK_UNREAD. */
    val kind: String = "SEND",
    /** For changes: the messages' `ferrowl://open` links. */
    val links: List<String> = emptyList(),
)

/**
 * The apps connected to Ferrowl (Ferrowl Bridge on Android), the activity
 * log and the messages waiting for confirmation, kept on the phone.
 */
@Singleton
class Connections @Inject constructor(@ApplicationContext context: Context) {
    private val prefs = context.getSharedPreferences("bridge", Context.MODE_PRIVATE)
    private val json = Json { ignoreUnknownKeys = true }

    private val _list = MutableStateFlow(read<List<Connection>>(KEY_LIST) ?: emptyList())
    val list: StateFlow<List<Connection>> = _list.asStateFlow()

    private val _log = MutableStateFlow(read<List<LogEntry>>(KEY_LOG) ?: emptyList())
    val log: StateFlow<List<LogEntry>> = _log.asStateFlow()

    private inline fun <reified T> read(key: String): T? =
        prefs.getString(key, null)?.let { runCatching { json.decodeFromString<T>(it) }.getOrNull() }

    private inline fun <reified T> write(key: String, value: T) =
        prefs.edit { putString(key, json.encodeToString(value)) }

    @Synchronized
    private fun save(list: List<Connection>) {
        _list.value = list
        write(KEY_LIST, list)
    }

    /** Connects an app; returns its new key. */
    @Synchronized
    fun add(packageName: String, label: String): Connection {
        val connection = Connection(token = newToken(), packageName = packageName, label = label)
        save(_list.value + connection)
        return connection
    }

    /** The connection of an app (the Tasker plugin connects it on first use: it may read). */
    @Synchronized
    fun forPackage(packageName: String, label: String): Connection =
        _list.value.firstOrNull { it.packageName == packageName } ?: add(packageName, label)

    @Synchronized
    fun update(connection: Connection) =
        save(_list.value.map { if (it.token == connection.token) connection else it })

    @Synchronized
    fun remove(token: String) = save(_list.value.filterNot { it.token == token })

    /** The app holding this key, if it is still connected. */
    fun find(token: String?): Connection? {
        val given = token?.trim()?.takeIf { it.isNotEmpty() }?.toByteArray() ?: return null
        // Compared in constant time: how long it takes says nothing about the key.
        return _list.value.firstOrNull { java.security.MessageDigest.isEqual(it.token.toByteArray(), given) }
    }

    @Synchronized
    fun used(connection: Connection, action: String, outcome: String) {
        val now = System.currentTimeMillis()
        _list.value.firstOrNull { it.token == connection.token }?.let { update(it.copy(lastUsed = now)) }
        val log = (listOf(LogEntry(now, connection.label, action, outcome)) + _log.value).take(LOG_SIZE)
        _log.value = log
        write(KEY_LOG, log)
    }

    @Synchronized
    fun addPending(send: PendingSend) {
        val pending = (read<List<PendingSend>>(KEY_PENDING) ?: emptyList()) + send
        write(KEY_PENDING, pending.takeLast(PENDING_SIZE))
    }

    /** Takes a waiting message out (once: answering twice does nothing). */
    @Synchronized
    fun takePending(id: String): PendingSend? {
        val pending = read<List<PendingSend>>(KEY_PENDING) ?: return null
        val found = pending.firstOrNull { it.id == id } ?: return null
        write(KEY_PENDING, pending.filterNot { it.id == id })
        return found
    }

    private fun newToken(): String {
        val bytes = ByteArray(18).also { SecureRandom().nextBytes(it) }
        return android.util.Base64.encodeToString(bytes, android.util.Base64.URL_SAFE or android.util.Base64.NO_WRAP or android.util.Base64.NO_PADDING)
    }

    companion object {
        private const val KEY_LIST = "connections"
        private const val KEY_LOG = "log"
        private const val KEY_PENDING = "pending"
        private const val LOG_SIZE = 50
        private const val PENDING_SIZE = 20

        /** Automation apps offered when connecting one (others by package name). */
        val KNOWN = listOf(
            "net.dinglisch.android.taskerm" to "Tasker",
            "com.arlosoft.macrodroid" to "MacroDroid",
            "com.llamalab.automate" to "Automate",
        )
    }
}
