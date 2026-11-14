package com.ferrowl.app.push

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.ferrowl.app.data.SecretBox
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.serialization.json.Json
import javax.inject.Inject
import javax.inject.Singleton

private val Context.push: DataStore<Preferences> by preferencesDataStore(name = "push")

/** What the phone knows about its registration on the push server. */
data class PushState(
    val enabled: Boolean = true,
    /** The user was told what instant notifications send to the server and answered. */
    val asked: Boolean = false,
    /** Secret token identifying this phone to the server. */
    val deviceToken: String? = null,
    /** The phone's key (base64), which seals the account on the server. */
    val key: String? = null,
    /** Accounts watched by the server: address (lowercase) -> account id there. */
    val accounts: Map<String, String> = emptyMap(),
    /** Per address: highest inbox UID already notified (no repeated notifications). */
    val lastNotified: Map<String, Long> = emptyMap(),
    /** Server account ids whose removal failed (offline): tried again at every sync. */
    val removing: Set<String> = emptySet(),
) {
    /** The address of an account id the server sent. */
    fun emailOf(accountId: String): String? = accounts.entries.firstOrNull { it.value == accountId }?.key
}

/**
 * Saves the push registration (Jetpack DataStore). The device token and the
 * key are secrets: they are encrypted with [SecretBox] (Android Keystore)
 * before saving.
 */
@Singleton
class PushStore @Inject constructor(
    @ApplicationContext private val context: Context,
    private val secrets: SecretBox,
) {
    val state: Flow<PushState> = context.push.data.map { p ->
        PushState(
            // On by default: the phone registers with the first account;
            // turning it off in Settings is remembered.
            enabled = p[ENABLED] ?: true,
            asked = p[ASKED] ?: false,
            deviceToken = p[TOKEN]?.let(::open),
            key = p[KEY]?.let(::open),
            accounts = p[ACCOUNTS]?.let { runCatching { Json.decodeFromString<Map<String, String>>(it) }.getOrNull() }.orEmpty(),
            lastNotified = p[LAST_UIDS]?.let { runCatching { Json.decodeFromString<Map<String, Long>>(it) }.getOrNull() }.orEmpty(),
            removing = p[REMOVING]?.let { runCatching { Json.decodeFromString<Set<String>>(it) }.getOrNull() }.orEmpty(),
        )
    }

    suspend fun current(): PushState = state.first()

    /** On or off, after the user answered. */
    suspend fun setEnabled(on: Boolean) = context.push.edit {
        it[ENABLED] = on
        it[ASKED] = true
    }

    suspend fun saveDevice(deviceToken: String, key: String) = context.push.edit {
        it[TOKEN] = secrets.encrypt(deviceToken)
        it[KEY] = secrets.encrypt(key)
    }

    /** The server watches this account under `id`. */
    suspend fun saveAccount(email: String, id: String) = editAccounts { it + (email.lowercase() to id) }

    suspend fun removeAccount(email: String) = editAccounts { it - email.lowercase() }

    /** An older version registered one account, without its address: forgotten (registered again). */
    suspend fun legacyAccount(): String? = context.push.data.first()[ACCOUNT]

    suspend fun clearLegacyAccount() = context.push.edit { it.remove(ACCOUNT) }

    /** Mail up to [uid] was announced or seen in the app; never goes back. */
    suspend fun setLastNotified(email: String, uid: Long) = context.push.edit { p ->
        val now = p[LAST_UIDS]?.let { runCatching { Json.decodeFromString<Map<String, Long>>(it) }.getOrNull() }.orEmpty()
        val key = email.lowercase()
        if (uid > (now[key] ?: 0)) p[LAST_UIDS] = Json.encodeToString(now + (key to uid))
    }

    /** An account id to remove from the server later (or done: [add] false). */
    suspend fun setRemoving(id: String, add: Boolean) = context.push.edit { p ->
        val now = p[REMOVING]?.let { runCatching { Json.decodeFromString<Set<String>>(it) }.getOrNull() }.orEmpty()
        p[REMOVING] = Json.encodeToString(if (add) now + id else now - id)
    }

    private suspend fun editAccounts(change: (Map<String, String>) -> Map<String, String>) = context.push.edit { p ->
        val now = p[ACCOUNTS]?.let { runCatching { Json.decodeFromString<Map<String, String>>(it) }.getOrNull() }.orEmpty()
        p[ACCOUNTS] = Json.encodeToString(change(now))
    }

    /** Forgets the registration (the switch stays as it is). */
    suspend fun clearRegistration() = context.push.edit {
        it.remove(TOKEN)
        it.remove(KEY)
        it.remove(ACCOUNT)
        it.remove(ACCOUNTS)
        it.remove(REMOVING)
    }

    /** An unreadable secret (e.g. after a backup restore) counts as missing. */
    private fun open(sealed: String): String? = runCatching { secrets.decrypt(sealed) }.getOrNull()

    private companion object {
        val ENABLED = booleanPreferencesKey("enabled")
        val ASKED = booleanPreferencesKey("asked")
        val TOKEN = stringPreferencesKey("device_token")
        val KEY = stringPreferencesKey("key")
        /** Older versions: the only account watched. */
        val ACCOUNT = stringPreferencesKey("account_id")
        val ACCOUNTS = stringPreferencesKey("accounts")
        val LAST_UIDS = stringPreferencesKey("last_notified_uids")
        val REMOVING = stringPreferencesKey("removing")
    }
}
