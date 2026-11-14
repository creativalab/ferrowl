package com.ferrowl.app

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.ferrowl.app.data.MailRepository
import com.ferrowl.app.push.ForegroundMailbox
import com.ferrowl.app.push.MailCheck
import dagger.hilt.android.HiltAndroidApp
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import javax.inject.Inject

/** Application entry point. `@HiltAndroidApp` starts dependency injection. */
@HiltAndroidApp
class FerrowlApp : Application() {
    @Inject lateinit var mail: MailRepository

    @Inject lateinit var mailCheck: MailCheck

    override fun onCreate() {
        super.onCreate()
        val process = ProcessLifecycleOwner.get()
        // Back from the background (the whole app, not a picker, sign-in or
        // purchase screen opened from it): the open list comes again from
        // the server. The first start is the launch itself.
        process.lifecycle.addObserver(object : DefaultLifecycleObserver {
            private var started = false

            override fun onStart(owner: LifecycleOwner) {
                ForegroundMailbox.foreground = true
                if (started) mail.returned()
                started = true
            }

            override fun onStop(owner: LifecycleOwner) {
                ForegroundMailbox.foreground = false
            }
        })
        process.lifecycleScope.launch {
            mail.activeEmail.collect { ForegroundMailbox.active = it }
        }
        // The periodic check in the background runs while there are accounts.
        process.lifecycleScope.launch {
            mail.accountList.map { it.isNotEmpty() }.distinctUntilChanged().collect { mailCheck.schedule(it) }
        }
        // While the app is open: every account's new mail every few minutes
        // (as iOS), also those the push server does not watch.
        process.lifecycleScope.launch {
            process.repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    delay(WATCH_MS)
                    mailCheck.checkAll(foreground = true)
                }
            }
        }
    }

    private companion object {
        const val WATCH_MS = 180_000L
    }
}
