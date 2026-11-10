package com.ferrowl.app.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.MutablePreferences
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import com.ferrowl.core.Account
import com.ferrowl.core.OAuthProvider
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import javax.inject.Inject
import javax.inject.Singleton

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "accounts")

/**
 * Saves the accounts on the phone (Jetpack DataStore). Passwords and
 * OpenPGP passphrases are encrypted with [SecretBox] before saving.
 *
 * Several accounts, one of them active (the one shown and connected).
 * Accounts are identified by their address. Data saved by older versions
 * (a single account) is read as a list of one.
 */
@Singleton
class AccountStore @Inject constructor(
    @ApplicationContext private val context: Context,
    private val secrets: SecretBox,
) {
    /** Every account, in the order they were added. */
    val accounts: Flow<List<Account>> = context.dataStore.data.map { prefs -> read(prefs).list }

    /** The active account, or null before the first setup. */
    val account: Flow<Account?> = context.dataStore.data.map { prefs -> read(prefs).active }

    /** Adds the account, or replaces the one with the same address; it becomes active. */
    suspend fun save(account: Account) {
        context.dataStore.edit { prefs ->
            val list = read(prefs).list.filterNot { it.email.equals(account.email, ignoreCase = true) } + account
            write(prefs, list, account.email)
        }
    }

    /** Changes an account's settings in place (its order and the active one stay). */
    suspend fun update(account: Account) {
        context.dataStore.edit { prefs ->
            val current = read(prefs)
            val list = current.list.map { if (it.email.equals(account.email, ignoreCase = true)) account else it }
            write(prefs, list, current.active?.email ?: account.email)
        }
    }

    /** Makes the account with this address the active one. */
    suspend fun setActive(email: String) {
        context.dataStore.edit { prefs ->
            val list = read(prefs).list
            if (list.any { it.email.equals(email, ignoreCase = true) }) write(prefs, list, email)
        }
    }

    /** Removes an account; the first remaining one becomes active. Returns the new active one. */
    suspend fun remove(email: String): Account? {
        var active: Account? = null
        context.dataStore.edit { prefs ->
            val current = read(prefs)
            val list = current.list.filterNot { it.email.equals(email, ignoreCase = true) }
            val keep = current.active?.takeUnless { it.email.equals(email, ignoreCase = true) } ?: list.firstOrNull()
            active = keep
            write(prefs, list, keep?.email)
        }
        return active
    }

    suspend fun clear() {
        context.dataStore.edit {
            it.remove(KEY)
            it.remove(LIST)
            it.remove(ACTIVE)
        }
    }

    private class Snapshot(val list: List<Account>, val active: Account?)

    private fun read(prefs: Preferences): Snapshot {
        val list = prefs[LIST]?.let { json ->
            runCatching { Json.decodeFromString<List<Stored>>(json) }.getOrNull()
        } ?: prefs[KEY]?.let { json ->
            // Saved by a version with a single account.
            runCatching { listOf(Json.decodeFromString<Stored>(json)) }.getOrNull()
        } ?: emptyList()
        val accounts = list.mapNotNull { runCatching { it.toAccount(secrets) }.getOrNull() }
        val wanted = prefs[ACTIVE]
        val active = accounts.firstOrNull { it.email.equals(wanted, ignoreCase = true) } ?: accounts.firstOrNull()
        return Snapshot(accounts, active)
    }

    private fun write(prefs: MutablePreferences, list: List<Account>, active: String?) {
        prefs[LIST] = Json.encodeToString(list.map { Stored.from(it, secrets) })
        if (active != null) prefs[ACTIVE] = active else prefs.remove(ACTIVE)
        prefs.remove(KEY)
    }

    /** What is written to disk: the engine's Account with secrets encrypted. */
    @Serializable
    private data class Stored(
        val displayName: String,
        val email: String,
        val username: String,
        val passwordSealed: String,
        val incomingHost: String,
        val incomingPort: Int,
        val smtpHost: String,
        val smtpPort: Int,
        val pop3: Boolean,
        val pop3KeepDays: Int? = null,
        val pgpPassphraseSealed: String = "",
        /** "MICROSOFT" / "GOOGLE" when the account signs in instead of a password. */
        val oauth: String? = null,
    ) {
        fun toAccount(secrets: SecretBox) = Account(
            displayName = displayName,
            email = email,
            username = username,
            password = if (passwordSealed.isEmpty()) "" else secrets.decrypt(passwordSealed),
            incomingHost = incomingHost,
            incomingPort = incomingPort.toUShort(),
            smtpHost = smtpHost,
            smtpPort = smtpPort.toUShort(),
            protocol = if (pop3) com.ferrowl.core.Protocol.POP3 else com.ferrowl.core.Protocol.IMAP,
            pop3KeepDays = pop3KeepDays?.toUInt(),
            pgpPassphrase = if (pgpPassphraseSealed.isEmpty()) "" else secrets.decrypt(pgpPassphraseSealed),
            oauth = oauth?.let { name -> OAuthProvider.entries.firstOrNull { it.name == name } },
        )

        companion object {
            fun from(a: Account, secrets: SecretBox) = Stored(
                displayName = a.displayName,
                email = a.email,
                username = a.username,
                passwordSealed = if (a.password.isEmpty()) "" else secrets.encrypt(a.password),
                incomingHost = a.incomingHost,
                incomingPort = a.incomingPort.toInt(),
                smtpHost = a.smtpHost,
                smtpPort = a.smtpPort.toInt(),
                pop3 = a.protocol == com.ferrowl.core.Protocol.POP3,
                pop3KeepDays = a.pop3KeepDays?.toInt(),
                pgpPassphraseSealed = if (a.pgpPassphrase.isEmpty()) "" else secrets.encrypt(a.pgpPassphrase),
                oauth = a.oauth?.name,
            )
        }
    }

    private companion object {
        /** Older versions: a single account. */
        val KEY = stringPreferencesKey("account")
        val LIST = stringPreferencesKey("accounts")
        val ACTIVE = stringPreferencesKey("active")
    }
}
