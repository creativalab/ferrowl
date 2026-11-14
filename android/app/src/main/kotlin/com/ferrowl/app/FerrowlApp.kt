package com.ferrowl.app

import android.app.Application
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.ferrowl.app.data.MailRepository
import dagger.hilt.android.HiltAndroidApp
import javax.inject.Inject

/** Application entry point. `@HiltAndroidApp` starts dependency injection. */
@HiltAndroidApp
class FerrowlApp : Application() {
    @Inject lateinit var mail: MailRepository

    override fun onCreate() {
        super.onCreate()
        // Back from the background (the whole app, not a picker, sign-in or
        // purchase screen opened from it): the open list comes again from
        // the server. The first start is the launch itself.
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            private var started = false

            override fun onStart(owner: LifecycleOwner) {
                if (started) mail.returned()
                started = true
            }
        })
    }
}
