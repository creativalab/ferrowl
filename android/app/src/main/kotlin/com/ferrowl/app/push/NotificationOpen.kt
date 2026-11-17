package com.ferrowl.app.push

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject
import javax.inject.Singleton

/** A tapped new-mail notification: the account and the message to open (null uid = its inbox). */
data class OpenRequest(val email: String, val uid: Long?)

/**
 * Hands a tapped notification from [com.ferrowl.app.MainActivity] to the
 * mailbox, which switches to the account if needed and opens the message.
 */
@Singleton
class NotificationOpen @Inject constructor() {
    private val _pending = MutableStateFlow<OpenRequest?>(null)
    val pending: StateFlow<OpenRequest?> = _pending.asStateFlow()

    fun request(email: String, uid: Long?) {
        _pending.value = OpenRequest(email, uid)
    }

    fun done() {
        _pending.value = null
    }

    /**
     * The message of a tapped notification, until it has loaded: the inbox's
     * fetch from the server waits for it (both go through the one
     * connection, and the list with its rules takes a while).
     */
    private val opening = MutableStateFlow<Long?>(null)

    fun opening(uid: Long?) {
        opening.value = uid
    }

    fun loaded(uid: Long) {
        opening.compareAndSet(uid, null)
    }

    /** Waits (a few seconds at most) for the notification's message to load. */
    suspend fun afterOpening() {
        withTimeoutOrNull(8_000) { opening.first { it == null } }
        opening.value = null
    }

    companion object {
        const val EXTRA_EMAIL = "com.ferrowl.app.open.email"
        const val EXTRA_UID = "com.ferrowl.app.open.uid"
    }
}
