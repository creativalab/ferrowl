package com.ferrowl.app.ui.compose

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.ferrowl.app.data.MailRepository
import com.ferrowl.app.ui.message.ReplyKind
import com.ferrowl.core.AddressSuggestion
import com.ferrowl.core.FerrowlException
import com.ferrowl.core.Outgoing
import com.ferrowl.core.forwardDraft
import com.ferrowl.core.replyDraft
import com.ferrowl.core.translate
import dagger.hilt.android.lifecycle.HiltViewModel
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID
import javax.inject.Inject

data class ComposeState(
    /** The sending account ("Name <address>"). */
    val from: String = "",
    val to: String = "",
    val cc: String = "",
    val bcc: String = "",
    val subject: String = "",
    val body: String = "",
    /** Threading headers and flags kept from the draft (replies). */
    val base: Outgoing? = null,
    val loading: Boolean = false,
    val sending: Boolean = false,
    val error: String? = null,
    val sent: Boolean = false,
    /** The user typed something: leaving asks before discarding. */
    val edited: Boolean = false,
    /** Suggestions for the word being typed in To (true) or Cc (false). */
    val suggestFor: Boolean? = null,
    val suggestions: List<AddressSuggestion> = emptyList(),
    /** Files to send, copied into the app's private storage. */
    val attachments: List<Attachment> = emptyList(),
)

/** A file to send: its name, size in bytes and the private copy that is sent. */
data class Attachment(val name: String, val size: Long, val path: String)

@HiltViewModel
class ComposeViewModel @Inject constructor(
    private val repo: MailRepository,
    @ApplicationContext private val context: Context,
    /** What was written, kept if Android stops the app meanwhile (e.g. while the camera is open). */
    private val saved: SavedStateHandle,
) : ViewModel() {
    private val _state = MutableStateFlow(restore() ?: ComposeState())
    val state: StateFlow<ComposeState> = _state.asStateFlow()

    /** The error was shown to the user (as a notice at the top). */
    fun errorShown() = _state.update { it.copy(error = null) }
    private var started = false

    /** The account the message goes out from (the one in use when it was started). */
    private var account: String? = saved[KEY_ACCOUNT]

    init {
        // Every change is kept, so a message restored after the app was
        // stopped is the one on screen (not an empty or quote-only body).
        viewModelScope.launch { _state.collect(::keep) }
    }

    private fun keep(s: ComposeState) {
        saved[KEY_SAVED] = true
        saved[KEY_TO] = s.to
        saved[KEY_CC] = s.cc
        saved[KEY_BCC] = s.bcc
        saved[KEY_SUBJECT] = s.subject
        saved[KEY_BODY] = s.body
        saved[KEY_EDITED] = s.edited
        saved[KEY_FILE_NAMES] = ArrayList(s.attachments.map { it.name })
        saved[KEY_FILE_SIZES] = s.attachments.map { it.size }.toLongArray()
        saved[KEY_FILE_PATHS] = ArrayList(s.attachments.map { it.path })
    }

    /** The message kept before the app was stopped, if any. */
    private fun restore(): ComposeState? {
        if (saved.get<Boolean>(KEY_SAVED) != true) return null
        val names = saved.get<ArrayList<String>>(KEY_FILE_NAMES).orEmpty()
        val sizes = saved.get<LongArray>(KEY_FILE_SIZES) ?: LongArray(0)
        val paths = saved.get<ArrayList<String>>(KEY_FILE_PATHS).orEmpty()
        val files = paths.indices.mapNotNull { i ->
            // The private copies stay in the cache (unless Android cleared it).
            Attachment(names.getOrElse(i) { "file" }, sizes.getOrElse(i) { 0 }, paths[i]).takeIf { File(it.path).exists() }
        }
        return ComposeState(
            to = saved[KEY_TO] ?: "",
            cc = saved[KEY_CC] ?: "",
            bcc = saved[KEY_BCC] ?: "",
            subject = saved[KEY_SUBJECT] ?: "",
            body = saved[KEY_BODY] ?: "",
            edited = saved[KEY_EDITED] ?: false,
            attachments = files,
        )
    }

    /**
     * Prepares a reply or forward from the original message, or a new
     * message to [to] (from the contacts), maybe with [cc], [subject] and
     * [body] (from a link or another app). Runs once.
     */
    fun start(
        kind: ReplyKind?,
        folderId: String,
        uid: UInt,
        to: String = "",
        cc: String = "",
        subject: String = "",
        body: String = "",
    ) {
        if (started) return
        started = true
        // Brought back after the app was stopped: the fields are the user's.
        val restored = saved.get<Boolean>(KEY_STARTED) == true
        saved[KEY_STARTED] = true
        viewModelScope.launch {
            repo.savedAccount()?.let { a ->
                if (account == null) {
                    account = a.email
                    saved[KEY_ACCOUNT] = a.email
                }
                val from = if (a.displayName.isBlank()) a.email else "${a.displayName} <${a.email}>"
                _state.update { it.copy(from = from) }
            }
        }
        if (kind == null) {
            if (!restored) _state.update { it.copy(to = to, cc = cc, subject = subject, body = body) }
            return
        }
        _state.update { it.copy(loading = true) }
        viewModelScope.launch {
            try {
                val original = repo.message(folderId, uid)
                val own = repo.savedAccount()?.email.orEmpty()
                val draft = when (kind) {
                    ReplyKind.REPLY -> replyDraft(original, false, own)
                    ReplyKind.REPLY_ALL -> replyDraft(original, true, own)
                    ReplyKind.FORWARD -> forwardDraft(original)
                }
                _state.update {
                    if (restored) {
                        // Only the threading headers: the text is what was written.
                        it.copy(base = draft, loading = false)
                    } else {
                        it.copy(to = draft.to, cc = draft.cc, subject = draft.subject, body = draft.body, base = draft, loading = false)
                    }
                }
            } catch (e: FerrowlException) {
                _state.update { it.copy(loading = false, error = e.message) }
            }
        }
    }

    fun setTo(v: String) {
        _state.update { it.copy(to = v, edited = true, error = null) }
        suggest(v, forTo = true)
    }

    fun setBcc(v: String) = _state.update { it.copy(bcc = v, edited = true) }

    fun setCc(v: String) {
        _state.update { it.copy(cc = v, edited = true) }
        suggest(v, forTo = false)
    }

    /** Looks up the word after the last comma (2+ letters). */
    private fun suggest(field: String, forTo: Boolean) {
        val word = field.substringAfterLast(',').substringAfterLast(';').trim()
        if (word.length < 2 || '<' in word) {
            _state.update { it.copy(suggestFor = null, suggestions = emptyList()) }
            return
        }
        viewModelScope.launch {
            val found = repo.suggest(word)
            _state.update { it.copy(suggestFor = forTo.takeIf { found.isNotEmpty() }, suggestions = found) }
        }
    }

    /** Replaces the word being typed with the chosen address. */
    fun pick(s: AddressSuggestion) = _state.update { st ->
        val entry = if (s.name.isBlank()) s.address else "${s.name} <${s.address}>"
        fun replace(field: String): String {
            val cut = maxOf(field.lastIndexOf(','), field.lastIndexOf(';'))
            val keep = if (cut >= 0) field.substring(0, cut + 1) + " " else ""
            return "$keep$entry, "
        }
        when (st.suggestFor) {
            true -> st.copy(to = replace(st.to), suggestFor = null, suggestions = emptyList())
            false -> st.copy(cc = replace(st.cc), suggestFor = null, suggestions = emptyList())
            null -> st
        }
    }
    fun setSubject(v: String) = _state.update { it.copy(subject = v, edited = true) }
    fun setBody(v: String) = _state.update { it.copy(body = v, edited = true) }

    /**
     * Adds files picked by the user. Each is copied into the app's private
     * storage (the engine reads files by path; picked documents are only
     * readable through their content address).
     */
    private var sharedAdded = false

    /** Files shared from another app: attached once (not again after a rotation). */
    fun addShared(uris: List<Uri>) {
        if (sharedAdded || saved.get<Boolean>(KEY_SHARED) == true) return
        sharedAdded = true
        saved[KEY_SHARED] = true
        // Only what another app hands over, never Ferrowl's own files.
        addFiles(uris.filter { it.scheme == "content" && it.authority != "${context.packageName}.files" })
    }

    fun addFiles(uris: List<Uri>) {
        if (uris.isEmpty()) return
        viewModelScope.launch {
            val added = withContext(Dispatchers.IO) { uris.mapNotNull(::copyIn) }
            _state.update { it.copy(attachments = it.attachments + added, edited = true) }
        }
    }

    fun removeAttachment(path: String) {
        File(path).parentFile?.deleteRecursively()
        _state.update { s -> s.copy(attachments = s.attachments.filterNot { it.path == path }) }
    }

    private fun copyIn(uri: Uri): Attachment? = runCatching {
        val resolver = context.contentResolver
        var name = "file"
        var size = -1L
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                c.getString(0)?.let { name = it }
                if (!c.isNull(1)) size = c.getLong(1)
            }
        }
        // One folder per file keeps the original name (two files may share it).
        val dir = File(context.cacheDir, "outgoing/${UUID.randomUUID()}").apply { mkdirs() }
        val file = File(dir, name.replace('/', '_'))
        resolver.openInputStream(uri)?.use { input -> file.outputStream().use { input.copyTo(it) } } ?: return@runCatching null
        Attachment(name, if (size >= 0) size else file.length(), file.path)
    }.getOrNull()

    /** The private copies are not needed once sent or discarded. */
    private fun dropCopies() {
        _state.value.attachments.forEach { File(it.path).parentFile?.deleteRecursively() }
    }

    override fun onCleared() {
        if (!_state.value.sending) dropCopies()
    }

    fun send() {
        val s = _state.value
        if (s.to.isBlank()) {
            _state.update { it.copy(error = translate("err_no_recipient")) }
            return
        }
        val draft = (s.base ?: Outgoing("", "", "", "", "", null, null, emptyList(), emptyList(), false, false))
            .copy(
                to = s.to.trim(),
                cc = s.cc.trim(),
                bcc = s.bcc.trim(),
                subject = s.subject.trim(),
                body = s.body,
                attachments = s.attachments.map { it.path },
            )
        _state.update { it.copy(sending = true, error = null) }
        viewModelScope.launch {
            try {
                repo.send(draft, account)
                dropCopies()
                _state.update { it.copy(sending = false, sent = true) }
            } catch (e: com.ferrowl.app.data.AccountChanged) {
                // Not sent from an account other than the one it was written in.
                _state.update { it.copy(sending = false) }
            } catch (e: FerrowlException) {
                _state.update { it.copy(sending = false, error = e.message) }
            }
        }
    }

    private companion object {
        const val KEY_SAVED = "compose.saved"
        const val KEY_STARTED = "compose.started"
        const val KEY_SHARED = "compose.shared"
        const val KEY_ACCOUNT = "compose.account"
        const val KEY_TO = "compose.to"
        const val KEY_CC = "compose.cc"
        const val KEY_BCC = "compose.bcc"
        const val KEY_SUBJECT = "compose.subject"
        const val KEY_BODY = "compose.body"
        const val KEY_EDITED = "compose.edited"
        const val KEY_FILE_NAMES = "compose.fileNames"
        const val KEY_FILE_SIZES = "compose.fileSizes"
        const val KEY_FILE_PATHS = "compose.filePaths"
    }
}
