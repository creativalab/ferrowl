package com.ferrowl.app.ui.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.data.AccountStore
import com.ferrowl.app.data.MailRepository
import com.ferrowl.app.data.SettingsStore
import com.ferrowl.app.push.PushRegistrar
import com.ferrowl.app.push.PushStore
import com.ferrowl.core.Account
import com.ferrowl.core.Protocol
import com.ferrowl.core.translate
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import javax.inject.Inject

@HiltViewModel
class SettingsViewModel @Inject constructor(
    private val settings: SettingsStore,
    private val push: PushRegistrar,
    pushStore: PushStore,
    accounts: AccountStore,
    private val repo: MailRepository,
) : ViewModel() {
    private val _blocked = MutableStateFlow<List<String>>(emptyList())

    /** Senders whose mail goes to Spam ("name@domain" or "@domain"). */
    val blocked: StateFlow<List<String>> = _blocked.asStateFlow()

    private val _allowed = MutableStateFlow<List<String>>(emptyList())

    /** Senders whose mail comes back from Spam. */
    val allowed: StateFlow<List<String>> = _allowed.asStateFlow()

    init {
        viewModelScope.launch {
            // The lists belong to the account: connect it first.
            runCatching { repo.ensureConnected() }
            reloadSenders()
        }
    }

    private suspend fun reloadSenders() {
        _blocked.value = repo.senderRules(false)
        _allowed.value = repo.senderRules(true)
    }

    /** Adds a sender to the blocked (false) or allowed (true) list. */
    fun addSender(address: String, allow: Boolean) {
        if (!address.contains('@')) return
        viewModelScope.launch {
            runCatching { repo.setSenderRule(address.trim(), allow) }
            reloadSenders()
        }
    }

    fun removeSender(address: String) {
        viewModelScope.launch {
            repo.removeSenderRule(address)
            reloadSenders()
        }
    }
    /** Conversations (true) or one row per message. */
    val threaded: StateFlow<Boolean> =
        settings.threaded.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), true)

    /** Whether instant notifications are on (as saved). */
    val pushEnabled: StateFlow<Boolean> =
        pushStore.state.map { it.enabled }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)

    /**
     * Why instant notifications are not available for the active account
     * (translation key), or null when they are: POP3 has no IDLE, and the
     * server cannot watch accounts that sign in with Microsoft/Google yet.
     */
    val pushUnavailable: StateFlow<String?> =
        accounts.account.map {
            when {
                it?.protocol == Protocol.POP3 -> "push_pop3"
                it?.oauth != null -> "push_oauth"
                else -> null
            }
        }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    /** Every account on the phone, in their order. */
    val accounts: StateFlow<List<Account>> =
        repo.accountList.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    /** The color theme (an id of design/themes.json, or "auto"). */
    val theme: StateFlow<String> = settings.theme.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), "auto")

    fun setTheme(id: String) {
        viewModelScope.launch { settings.setTheme(id) }
    }

    /** Pictures in HTML mail load from the network. */
    val remoteImages: StateFlow<Boolean> =
        settings.remoteImages.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), true)

    fun setRemoteImages(on: Boolean) {
        viewModelScope.launch { settings.setRemoteImages(on) }
    }

    /** Saves an account's name and contacts/calendar servers. */
    fun updateAccount(account: Account) {
        viewModelScope.launch {
            runCatching { repo.updateAccount(account) }.onFailure { _pushError.value = it.message }
        }
    }

    /** Removes an account from the phone (mail on the server is not touched). */
    fun removeAccount(email: String) {
        viewModelScope.launch {
            val left = runCatching { repo.removeAccount(email) }.getOrDefault(true)
            if (!left) _noAccount.value = true
        }
    }

    /** Address of the active account (for "Remove account"). */
    val activeEmail: StateFlow<String> =
        accounts.account.map { it?.email.orEmpty() }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), "")

    /** Set after removing the last account: the app goes back to setup. */
    private val _noAccount = MutableStateFlow(false)
    val noAccount: StateFlow<Boolean> = _noAccount.asStateFlow()

    /** Removes the active account from the phone (mail on the server is not touched). */
    fun removeActiveAccount() {
        val email = activeEmail.value.ifEmpty { return }
        viewModelScope.launch {
            val left = runCatching { repo.removeAccount(email) }.getOrDefault(true)
            if (!left) _noAccount.value = true
        }
    }

    private val _pushBusy = MutableStateFlow(false)
    val pushBusy: StateFlow<Boolean> = _pushBusy.asStateFlow()

    private val _pushError = MutableStateFlow<String?>(null)
    val pushError: StateFlow<String?> = _pushError.asStateFlow()

    fun pushErrorShown() {
        _pushError.value = null
    }

    fun setThreaded(on: Boolean) {
        viewModelScope.launch { settings.setThreaded(on) }
    }

    /** Turns instant notifications on (registers with the server) or off. */
    fun setPush(on: Boolean) {
        if (_pushBusy.value) return
        _pushBusy.value = true
        _pushError.value = null
        viewModelScope.launch {
            try {
                if (on) push.enable() else push.disable()
            } catch (e: Exception) {
                _pushError.value = translate("push_error").replace("{err}", e.message.orEmpty())
            } finally {
                _pushBusy.value = false
            }
        }
    }
}
