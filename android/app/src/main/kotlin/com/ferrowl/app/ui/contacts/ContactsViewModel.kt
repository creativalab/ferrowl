package com.ferrowl.app.ui.contacts

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.data.MailRepository
import com.ferrowl.core.Contact
import com.ferrowl.core.translate
import com.ferrowl.core.FerrowlException
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ContactsState(
    val query: String = "",
    val contacts: List<Contact> = emptyList(),
    /** A sync with the server is running. */
    val syncing: Boolean = false,
    /** Why the last sync failed (shown under the search field). */
    val error: String? = null,
    /** The contact shown in the bottom sheet. */
    val open: Contact? = null,
    /** The contact being written (new: empty url). */
    val editing: Contact? = null,
)

@HiltViewModel
class ContactsViewModel @Inject constructor(
    private val repo: MailRepository,
    private val notices: com.ferrowl.app.ui.notice.Notices,
) : ViewModel() {
    private val _state = MutableStateFlow(ContactsState())
    val state: StateFlow<ContactsState> = _state.asStateFlow()

    /** The error was shown to the user (as a notice at the top). */
    fun errorShown() = _state.update { it.copy(error = null) }
    private var search: Job? = null

    init {
        // The saved copy right away; the app already synced after connecting.
        reload()
    }

    fun setQuery(q: String) {
        _state.update { it.copy(query = q) }
        reload()
    }

    /** Reads the contacts matching the query from the phone's copy. */
    private fun reload() {
        search?.cancel()
        search = viewModelScope.launch {
            val list = repo.contacts(_state.value.query)
            _state.update { it.copy(contacts = list) }
        }
    }

    /** Asks the server for changes (the sync button). */
    fun sync() {
        if (_state.value.syncing) return
        _state.update { it.copy(syncing = true, error = null) }
        viewModelScope.launch {
            try {
                repo.ensureConnected()
                repo.syncContacts()
                _state.update { it.copy(syncing = false) }
            } catch (e: FerrowlException) {
                _state.update { it.copy(syncing = false, error = e.message) }
            }
            reload()
        }
    }

    fun open(c: Contact?) = _state.update { it.copy(open = c) }

    fun edit(c: Contact?) = _state.update { it.copy(editing = c, open = null) }

    fun newContact() = edit(Contact("", "", emptyList(), emptyList(), "", ""))

    /** Saves the contact being written (on the server, or on the phone). */
    fun save(c: Contact) {
        viewModelScope.launch {
            try {
                repo.saveContact(c)
                notices.info(translate("contact_saved"))
                _state.update { it.copy(editing = null) }
                reload()
            } catch (e: FerrowlException) {
                _state.update { it.copy(error = e.message) }
            }
        }
    }

    fun delete(c: Contact) {
        viewModelScope.launch {
            try {
                repo.deleteContact(c.url)
                notices.info(translate("contact_deleted"))
                _state.update { it.copy(editing = null) }
                reload()
            } catch (e: FerrowlException) {
                _state.update { it.copy(error = e.message) }
            }
        }
    }
}
