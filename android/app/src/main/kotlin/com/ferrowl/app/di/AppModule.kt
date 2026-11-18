package com.ferrowl.app.di

import android.content.Context
import com.ferrowl.app.data.GoogleSignIn
import com.ferrowl.app.data.KeystoreTokenVault
import com.ferrowl.app.demo.Demo
import com.ferrowl.app.demo.DemoEngine
import com.ferrowl.core.Engine
import com.ferrowl.core.setAccessTokenSource
import com.ferrowl.core.setTokenVault
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import java.util.Locale
import javax.inject.Named
import javax.inject.Singleton

/** How Hilt builds objects that are not ours (here: the Rust engine). */
@Module
@InstallIn(SingletonComponent::class)
object AppModule {
    /**
     * One engine per app process, with its files in the app's private
     * storage. Sign-in tokens go to the Keystore-backed vault; Google
     * tokens come from Google Identity Services.
     */
    @Provides
    @Singleton
    fun engine(@ApplicationContext context: Context, vault: KeystoreTokenVault, google: GoogleSignIn): Engine {
        // Demo mode: an engine on an empty folder of its own, never connected.
        if (Demo.on) return DemoEngine(context.cacheDir.resolve("demo").path, Locale.getDefault().language)
        setTokenVault(vault)
        setAccessTokenSource(google)
        return Engine(dataDir = context.filesDir.resolve("mail").path, language = Locale.getDefault().language)
    }

    /**
     * A second engine on the same files, for the accounts that are not the
     * active one (new-mail notifications): connecting one there does not
     * disconnect the account on screen.
     */
    @Provides
    @Singleton
    @Named("side")
    fun sideEngine(
        @ApplicationContext context: Context,
        // Asked for so it is made first: it hands the engine the token vault.
        @Suppress("UNUSED_PARAMETER") main: Engine,
    ): Engine {
        if (Demo.on) return DemoEngine(context.cacheDir.resolve("demo").path, Locale.getDefault().language)
        return Engine(dataDir = context.filesDir.resolve("mail").path, language = Locale.getDefault().language)
    }
}
