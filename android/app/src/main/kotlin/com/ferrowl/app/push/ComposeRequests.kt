package com.ferrowl.app.push

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/** A new message asked for by another app: shown to the user, never sent by itself. */
data class ComposeRequest(
    val to: String = "",
    val cc: String = "",
    val subject: String = "",
    val body: String = "",
)

/**
 * Hands a `ferrowl://compose` or `mailto:` link (or text shared to Ferrowl)
 * from [com.ferrowl.app.MainActivity] to the mailbox, which opens the
 * composer once an account is ready.
 */
@Singleton
class ComposeRequests @Inject constructor() {
    private val _pending = MutableStateFlow<ComposeRequest?>(null)
    val pending: StateFlow<ComposeRequest?> = _pending.asStateFlow()

    fun request(request: ComposeRequest) {
        _pending.value = request
    }

    fun done() {
        _pending.value = null
    }
}
