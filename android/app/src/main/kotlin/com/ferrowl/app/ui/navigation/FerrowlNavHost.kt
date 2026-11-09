package com.ferrowl.app.ui.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.toRoute
import com.ferrowl.app.push.ComposeRequests
import com.ferrowl.app.ui.calendar.CalendarScreen
import com.ferrowl.app.ui.calendar.EventEditScreen
import com.ferrowl.app.ui.compose.ComposeScreen
import com.ferrowl.app.ui.contacts.ContactsScreen
import com.ferrowl.app.ui.conversation.ConversationScreen
import com.ferrowl.app.ui.mailbox.MailboxScreen
import com.ferrowl.app.ui.mailbox.MailboxViewModel
import com.ferrowl.app.ui.message.MessageScreen
import com.ferrowl.app.ui.message.ReplyKind
import com.ferrowl.app.ui.settings.SettingsScreen
import com.ferrowl.app.ui.setup.SetupScreen
import com.ferrowl.app.ui.start.StartScreen
import com.ferrowl.app.ui.start.StartViewModel
import kotlinx.serialization.Serializable

// Type-safe destinations (Navigation Compose): each screen is a class, and
// its arguments are its fields.
@Serializable object Start
@Serializable object Setup
@Serializable object Mailbox
@Serializable object Settings
@Serializable object Contacts
@Serializable object Agenda

/** The event form: `url` of the event to change, or null for a new one on `day` (YYYY-MM-DD). */
@Serializable data class EditEvent(val url: String? = null, val day: String = "")
/** A message; `starred` comes from the list (the message itself does not carry flags). */
@Serializable data class Read(val folderId: String, val uid: Long, val starred: Boolean = false)

/** A conversation; `uids` are comma-separated, oldest first. */
@Serializable data class Thread(val folderId: String, val uids: String)

/**
 * Write a message; `kind` is a [ReplyKind] name, or null for a new one.
 * `to` fills in the recipient of a new message (from the contacts); the
 * other fields come from a link or another app.
 */
@Serializable data class Write(
    val kind: String? = null,
    val folderId: String = "",
    val uid: Long = 0,
    val to: String = "",
    // A draft asked for by another app or a link (new messages only).
    val cc: String = "",
    val subject: String = "",
    val body: String = "",
)

@Composable
fun FerrowlNavHost(composeRequests: ComposeRequests) {
    val nav = rememberNavController()
    NavHost(navController = nav, startDestination = Start) {
        // Decides where to go: account setup the first time, mailbox after.
        composable<Start> {
            val vm: StartViewModel = hiltViewModel()
            StartScreen(vm) { hasAccount ->
                nav.navigate(if (hasAccount) Mailbox else Setup) { popUpTo(Start) { inclusive = true } }
            }
        }
        composable<Setup> {
            // First account, or one more from the mailbox: either way the
            // mailbox starts over with the new (active) account.
            SetupScreen(onDone = { nav.navigate(Mailbox) { popUpTo(nav.graph.id) { inclusive = true } } })
        }
        composable<Mailbox> {
            // A link or another app asked for a new message: once the
            // mailbox (an account) is there.
            val compose by composeRequests.pending.collectAsState()
            LaunchedEffect(compose) {
                compose?.let { r ->
                    composeRequests.done()
                    nav.navigate(Write(to = r.to, cc = r.cc, subject = r.subject, body = r.body))
                }
            }
            MailboxScreen(
                onOpen = { folderId, uid, starred -> nav.navigate(Read(folderId, uid.toLong(), starred)) },
                onOpenConversation = { folderId, uids -> nav.navigate(Thread(folderId, uids.joinToString(","))) },
                onCompose = { nav.navigate(Write()) },
                onSettings = { nav.navigate(Settings) },
                onContacts = { nav.navigate(Contacts) },
                onCalendar = { nav.navigate(Agenda) },
                onAddAccount = { nav.navigate(Setup) },
                onReply = { folderId, uid -> nav.navigate(Write(ReplyKind.REPLY.name, folderId, uid.toLong())) },
            )
        }
        composable<Settings> {
            SettingsScreen(
                onBack = { nav.popBackStack() },
                onNoAccount = { nav.navigate(Setup) { popUpTo(nav.graph.id) { inclusive = true } } },
            )
        }
        composable<Agenda> {
            CalendarScreen(
                onBack = { nav.popBackStack() },
                onNewEvent = { day -> nav.navigate(EditEvent(day = day.toString())) },
                onEditEvent = { url -> nav.navigate(EditEvent(url = url)) },
            )
        }
        composable<EditEvent> { EventEditScreen(onClose = { nav.popBackStack() }) }
        composable<Contacts> {
            ContactsScreen(
                onBack = { nav.popBackStack() },
                onWrite = { recipient -> nav.navigate(Write(to = recipient)) },
            )
        }
        composable<Thread> { entry ->
            val route = entry.toRoute<Thread>()
            ConversationScreen(
                folderId = route.folderId,
                uids = route.uids.split(',').mapNotNull { it.toUIntOrNull() },
                onBack = { nav.popBackStack() },
                onOpen = { uid -> nav.navigate(Read(route.folderId, uid.toLong())) },
            )
        }
        composable<Read> { entry ->
            val route = entry.toRoute<Read>()
            // The message list under this screen: the trash button removes the
            // message there with "Moved to Trash · Undo", as a swipe does.
            val listEntry = remember(entry) { runCatching { nav.getBackStackEntry(Mailbox) }.getOrNull() }
            val list: MailboxViewModel? = listEntry?.let { hiltViewModel(it) }
            val listFolder = list?.state?.collectAsState()?.value?.folderId
            MessageScreen(
                folderId = route.folderId,
                uid = route.uid.toUInt(),
                starred = route.starred,
                onBack = { nav.popBackStack() },
                onReply = { kind -> nav.navigate(Write(kind.name, route.folderId, route.uid)) },
                onGone = { nav.popBackStack(Mailbox, inclusive = false) },
                onTrash = list?.takeIf { listFolder == route.folderId }?.let { vm ->
                    {
                        vm.deleteRow(listOf(route.uid.toUInt()))
                        nav.popBackStack(Mailbox, inclusive = false)
                    }
                },
            )
        }
        composable<Write> { entry ->
            val route = entry.toRoute<Write>()
            ComposeScreen(
                kind = route.kind?.let(ReplyKind::valueOf),
                folderId = route.folderId,
                uid = route.uid.toUInt(),
                to = route.to,
                cc = route.cc,
                subject = route.subject,
                body = route.body,
                onClose = { nav.popBackStack() },
            )
        }
    }
}
