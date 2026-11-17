package com.ferrowl.app.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.ferrowl.app.demo.Demo
import com.ferrowl.core.SortOrder
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import javax.inject.Inject
import javax.inject.Singleton

private val Context.settings: DataStore<Preferences> by preferencesDataStore(name = "settings")

/**
 * App preferences that are not about an account. Demo mode: fixed ones
 * (light theme, remote pictures blocked), never saved.
 */
@Singleton
class SettingsStore @Inject constructor(@ApplicationContext private val context: Context) {
    /** Lists grouped into conversations (true, the default) or one row per message. */
    val threaded: Flow<Boolean> = if (Demo.on) flowOf(true) else context.settings.data.map { it[THREADED] ?: true }

    suspend fun setThreaded(on: Boolean) {
        if (Demo.on) return
        context.settings.edit { it[THREADED] = on }
    }

    /** Order of message lists (newest first by default). */
    val sort: Flow<SortOrder> = if (Demo.on) {
        flowOf(SortOrder.NEWEST)
    } else {
        context.settings.data.map { prefs ->
            prefs[SORT]?.let { name -> SortOrder.entries.firstOrNull { it.name == name } } ?: SortOrder.NEWEST
        }
    }

    suspend fun setSort(order: SortOrder) {
        if (Demo.on) return
        context.settings.edit { it[SORT] = order.name }
    }

    /** Color theme: an id of design/themes.json, or "auto" (light/dark as the system). */
    val theme: Flow<String> = if (Demo.on) flowOf("ruggine") else context.settings.data.map { it[THEME] ?: "auto" }

    suspend fun setTheme(id: String) {
        if (Demo.on) return
        context.settings.edit { it[THEME] = id }
    }

    /** Pictures in HTML mail load from the network (on by default, as on iOS and desktop). */
    val remoteImages: Flow<Boolean> =
        if (Demo.on) flowOf(false) else context.settings.data.map { it[REMOTE_IMAGES] ?: true }

    suspend fun setRemoteImages(on: Boolean) {
        if (Demo.on) return
        context.settings.edit { it[REMOTE_IMAGES] = on }
    }

    private companion object {
        val THREADED = booleanPreferencesKey("threaded")
        val SORT = stringPreferencesKey("sort")
        val THEME = stringPreferencesKey("theme")
        val REMOTE_IMAGES = booleanPreferencesKey("remote_images")
    }
}
