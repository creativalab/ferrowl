package com.ferrowl.app.bridge

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import dagger.hilt.android.AndroidEntryPoint
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject

/** Work that outlives a receiver's onReceive (see [BroadcastReceiver.goAsync]). */
internal val bridgeScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

/** Requests from automation apps ([Bridge]): exported, checked by key. */
@AndroidEntryPoint
class BridgeReceiver : BroadcastReceiver() {
    @Inject lateinit var bridge: Bridge

    override fun onReceive(context: Context, intent: Intent) {
        val done = goAsync()
        bridgeScope.launch {
            try {
                // Within the time Android gives a background broadcast.
                withTimeoutOrNull(TIMEOUT_MS) { bridge.handle(intent) }
            } finally {
                done.finish()
            }
        }
    }

    private companion object {
        const val TIMEOUT_MS = 45_000L
    }
}

/** The user's answer to "send this message?" (not exported: only Ferrowl's notification). */
@AndroidEntryPoint
class BridgeAnswerReceiver : BroadcastReceiver() {
    @Inject lateinit var bridge: Bridge

    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getStringExtra(EXTRA_ID) ?: return
        val allow = intent.getBooleanExtra(EXTRA_ALLOW, false)
        val done = goAsync()
        bridgeScope.launch {
            try {
                withTimeoutOrNull(45_000L) { bridge.answer(id, allow) }
            } finally {
                done.finish()
            }
        }
    }

    companion object {
        const val EXTRA_ID = "id"
        const val EXTRA_ALLOW = "allow"
    }
}
