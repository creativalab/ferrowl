package com.ferrowl.app.ui.message

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.data.MailRepository
import com.ferrowl.app.ui.notice.Notices
import com.ferrowl.core.EventDraft
import com.ferrowl.core.FerrowlException
import com.ferrowl.core.InviteReply
import com.ferrowl.core.MailInsight
import com.ferrowl.core.Message
import com.ferrowl.core.Repeat
import com.ferrowl.core.mailInsights
import com.ferrowl.core.translate
import dagger.hilt.android.lifecycle.HiltViewModel
import javax.inject.Inject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

sealed interface MessageState {
    data object Loading : MessageState
    data class Shown(val message: Message) : MessageState
    data class Failed(val error: String) : MessageState
    /** Archived or deleted: the screen closes. */
    data object Gone : MessageState
}

/** Our side of the message's invitation. */
data class InviteState(
    val reply: InviteReply? = null,
    /** A cancelled event that is still in the calendar. */
    val inCalendar: Boolean = false,
    /** An answer is being sent. */
    val busy: Boolean = false,
)

@HiltViewModel
class MessageViewModel @Inject constructor(
    private val repo: MailRepository,
    private val notices: Notices,
) : ViewModel() {
    private val _state = MutableStateFlow<MessageState>(MessageState.Loading)
    val state: StateFlow<MessageState> = _state.asStateFlow()
    private var folderId = ""
    private var uid = 0u
    private val _invite = MutableStateFlow(InviteState())
    val invite: StateFlow<InviteState> = _invite.asStateFlow()

    private val _starred = MutableStateFlow(false)

    /** The message's star (IMAP \\Flagged). */
    val starred: StateFlow<Boolean> = _starred.asStateFlow()

    private val _insights = MutableStateFlow<List<MailInsight>>(emptyList())

    /** What the message is about (a shipment, a code, a bill). */
    val insights: StateFlow<List<MailInsight>> = _insights.asStateFlow()

    /** An all-day reminder on a bill's due date, in the account's first calendar. */
    fun addDue(due: String, number: String, amount: String) {
        viewModelScope.launch {
            try {
                val calendar = repo.calendars().firstOrNull() ?: return@launch notices.error(translate("err_no_calendar"))
                val what = listOf(number, amount).filter { it.isNotEmpty() }.joinToString(" · ").ifEmpty { translate("insight_bill") }
                repo.saveEvent(
                    calendar.url,
                    null,
                    EventDraft(translate("insight_due_title").replace("{what}", what), "", "", true, "${due}T09:00", "${due}T10:00", Repeat.NEVER),
                )
                notices.info(translate("insight_due_added"))
            } catch (e: FerrowlException) {
                notices.error(e.message.orEmpty())
            }
        }
    }

    fun load(folderId: String, uid: UInt, starred: Boolean = false) {
        if (this.folderId == folderId && this.uid == uid) return
        this.folderId = folderId
        this.uid = uid
        _starred.value = starred
        viewModelScope.launch {
            _state.value = try {
                val message = repo.message(folderId, uid)
                _insights.value = withContext(Dispatchers.Default) { mailInsights(message) }
                MessageState.Shown(message)
            } catch (e: FerrowlException) {
                MessageState.Failed(e.message ?: "")
            }
            refreshInvite()
        }
    }

    /** Reads our earlier answer to the message's invitation, if any. */
    private suspend fun refreshInvite() {
        val inv = (_state.value as? MessageState.Shown)?.message?.invitation ?: return
        _invite.value = InviteState(
            reply = repo.invitationReply(inv.uid),
            inCalendar = inv.method == "CANCEL" && repo.inCalendar(inv.uid),
        )
    }

    /** Accept / Maybe / Decline. */
    fun respond(reply: InviteReply) {
        val calendar = (_state.value as? MessageState.Shown)?.message?.calendar ?: return
        if (_invite.value.busy) return
        _invite.update { it.copy(busy = true) }
        viewModelScope.launch {
            val notice = try {
                repo.respondInvitation(calendar, reply)
            } catch (e: FerrowlException) {
                e.message.orEmpty()
            }
            refreshInvite()
            _invite.update { it.copy(busy = false) }
            notices.info(notice)
        }
    }

    /** Removes the cancelled event from the calendar. */
    fun removeCancelled() {
        val uid = (_state.value as? MessageState.Shown)?.message?.invitation?.uid ?: return
        _invite.update { it.copy(busy = true) }
        viewModelScope.launch {
            val notice = try {
                repo.removeCancelled(uid)
            } catch (e: FerrowlException) {
                e.message.orEmpty()
            }
            refreshInvite()
            _invite.update { it.copy(busy = false) }
            notices.info(notice)
        }
    }

    /** True when Delete removes for good (the message is in the Trash). */
    val deletesForever: Boolean get() = repo.deletesForever(folderId)

    fun archive() = act { repo.archive(folderId, uid) }

    fun delete() = act { repo.delete(folderId, uid) }

    fun markUnread() = act { repo.setSeen(folderId, uid, false) }

    /** Whether the message is in Spam ("Not spam" instead of "Report spam"). */
    val inSpam: Boolean get() = repo.isSpam(folderId)

    /** Report spam, or in Spam "not spam" (which also allows the sender). */
    fun spam() {
        val toSpam = !inSpam
        val from = (_state.value as? MessageState.Shown)?.message?.fromAddress.orEmpty()
        act {
            if (!toSpam && from.isNotBlank()) repo.setSenderRule(from, true)
            repo.spam(folderId, listOf(uid), toSpam)
        }
    }

    /** Blocks the sender: the address goes to the blocked list, the message to Spam. */
    fun blockSender() {
        val from = (_state.value as? MessageState.Shown)?.message?.fromAddress.orEmpty()
        if (from.isBlank()) return
        act {
            repo.setSenderRule(from, false)
            if (!inSpam) repo.spam(folderId, listOf(uid), true)
        }
    }

    /** Stars or unstars the message (the screen stays open). */
    fun toggleStar() {
        val on = !_starred.value
        _starred.value = on
        viewModelScope.launch {
            try {
                repo.setFlagged(folderId, listOf(uid), on)
            } catch (e: FerrowlException) {
                _starred.value = !on
                notices.error(e.message.orEmpty())
            }
        }
    }

    private fun act(action: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                action()
                _state.value = MessageState.Gone
            } catch (e: FerrowlException) {
                _state.value = MessageState.Failed(e.message ?: "")
            }
        }
    }
}
