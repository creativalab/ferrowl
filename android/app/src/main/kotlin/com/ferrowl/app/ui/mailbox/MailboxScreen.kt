package com.ferrowl.app.ui.mailbox

import android.text.format.DateUtils
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.R
import com.ferrowl.app.ui.fill
import com.ferrowl.app.ui.notice.LocalBilling
import com.ferrowl.app.ui.notice.ReportError
import com.ferrowl.core.Folder
import com.ferrowl.core.FolderKind
import com.ferrowl.core.Header
import com.ferrowl.core.SortOrder
import kotlinx.coroutines.launch
import kotlin.math.abs
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material3.SuggestionChip
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.TextRange
import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Surface
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/**
 * The mail list of one folder, with the folder list in a side drawer,
 * search in the top bar and a button to write a new message.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MailboxScreen(
    /** Opens a message; `starred` lets the message screen show its star. */
    onOpen: (folderId: String, uid: UInt, starred: Boolean) -> Unit,
    onOpenConversation: (folderId: String, uids: List<UInt>) -> Unit,
    onCompose: () -> Unit,
    onSettings: () -> Unit,
    onContacts: () -> Unit,
    onCalendar: () -> Unit,
    onAddAccount: () -> Unit,
    onReply: (folderId: String, uid: UInt) -> Unit,
    vm: MailboxViewModel = hiltViewModel(),
) {
    val s by vm.state.collectAsStateWithLifecycle()
    // A tapped new-mail notification opens its message (after switching account).
    val openRequest by vm.openRequests.collectAsStateWithLifecycle()
    LaunchedEffect(openRequest, s.active) {
        if (openRequest == null) return@LaunchedEffect
        val (folder, uid) = vm.takeOpenRequest() ?: return@LaunchedEffect
        if (uid != null) onOpen(folder, uid, false)
    }
    val drawer = rememberDrawerState(DrawerValue.Closed)
    val scope = rememberCoroutineScope()
    // Instant notifications: what they send to the server is explained once,
    // with the first account, before anything is sent; Android 13+ then asks
    // for the permission to show them.
    val askPush by vm.askPush.collectAsStateWithLifecycle()
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { vm.enablePush() }
    val context = LocalContext.current
    if (askPush) {
        AlertDialog(
            onDismissRequest = {},
            title = { Text(stringResource(R.string.push_title)) },
            text = { Text(stringResource(R.string.push_desc)) },
            confirmButton = {
                TextButton(onClick = {
                    val needsPermission = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                    if (needsPermission) permission.launch(Manifest.permission.POST_NOTIFICATIONS) else vm.enablePush()
                }) { Text(stringResource(R.string.push_enable)) }
            },
            dismissButton = { TextButton(onClick = vm::declinePush) { Text(stringResource(R.string.pro_not_now)) } },
        )
    }
    var searchOpen by remember { mutableStateOf(false) }
    // The search text with its cursor: a command tapped below puts it at the end.
    var searchField by remember { mutableStateOf(TextFieldValue("")) }
    fun search(v: TextFieldValue) {
        searchField = v
        vm.setQuery(v.text)
    }
    // A failed refresh is reported at the top, like every notice.
    ReportError(s.error, vm::errorShown)
    if (s.confirmDelete) ConfirmBulkDelete(vm)
    // Switching account: connecting can take a few seconds.
    s.switching?.let { email ->
        // A dialog: always above the screen and its drawer; it cannot be dismissed.
        Dialog(onDismissRequest = {}, properties = DialogProperties(dismissOnBackPress = false, dismissOnClickOutside = false)) {
            Surface(shape = RoundedCornerShape(18.dp), tonalElevation = 6.dp, shadowElevation = 6.dp) {
                Column(Modifier.padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator()
                    Spacer(Modifier.height(12.dp))
                    Text(stringResource(R.string.status_connecting), style = MaterialTheme.typography.titleMedium)
                    Text(email, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
    if (s.showPro) ProOffer(s.proPrice, vm)
    LaunchedEffect(s.openAddAccount) {
        if (s.openAddAccount) {
            vm.addAccountOpened()
            onAddAccount()
        }
    }

    ModalNavigationDrawer(
        drawerState = drawer,
        drawerContent = {
            ModalDrawerSheet {
                // Scrollable list inside the sheet: some accounts have dozens of folders.
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    // The app's name with its logo, as on iOS.
                    Row(
                        Modifier.padding(start = 28.dp, top = 20.dp, bottom = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Image(
                            painterResource(R.drawable.logo),
                            contentDescription = null,
                            modifier = Modifier.size(36.dp).clip(RoundedCornerShape(9.dp)),
                        )
                        Spacer(Modifier.width(10.dp))
                        Text("Ferrowl", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                    }
                    // Accounts first: tap one to show it, or add another.
                    Text(
                        stringResource(R.string.section_account),
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(start = 28.dp, top = 20.dp, bottom = 8.dp),
                    )
                    val accountCounts by vm.accountCounts.collectAsStateWithLifecycle()
                    s.accounts.forEach { email ->
                        // Unread mail in its inbox, live for the account in use.
                        val active = email.equals(s.active, ignoreCase = true)
                        val unread = (if (active) vm.activeUnread() else null) ?: accountCounts[email.lowercase()] ?: 0
                        NavigationDrawerItem(
                            label = { Text(email, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            badge = { if (unread > 0) Text(unread.toString()) },
                            selected = email.equals(s.active, ignoreCase = true),
                            onClick = {
                                vm.switchAccount(email)
                                scope.launch { drawer.close() }
                            },
                            modifier = Modifier.padding(horizontal = 12.dp),
                        )
                    }
                    NavigationDrawerItem(
                        label = { Text(stringResource(R.string.add_account)) },
                        icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                        selected = false,
                        onClick = {
                            scope.launch { drawer.close() }
                            if (vm.mayAddAccount()) onAddAccount()
                        },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    // Contacts and calendar on top: the folder list can be long.
                    NavigationDrawerItem(
                        label = { Text(stringResource(R.string.contacts)) },
                        selected = false,
                        onClick = {
                            scope.launch { drawer.close() }
                            onContacts()
                        },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                    NavigationDrawerItem(
                        label = { Text(stringResource(R.string.calendar)) },
                        selected = false,
                        onClick = {
                            scope.launch { drawer.close() }
                            onCalendar()
                        },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                    Text(
                        stringResource(R.string.section_folders),
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(start = 28.dp, top = 24.dp, bottom = 12.dp),
                    )
                    // As on the desktop: Inbox and the special folders first,
                    // then the others (sorted by the server's names).
                    s.folders.filter { it.selectable }.sortedBy { kindRank(it.kind) }.forEach { f ->
                        val count = s.unread[f.id]?.toInt() ?: 0
                        // Subfolders are indented under their parent (special
                        // folders are not, even under Gmail's "[Gmail]").
                        val depth = if (f.kind == FolderKind.OTHER) (f.path.size - 1).coerceAtLeast(0) else 0
                        NavigationDrawerItem(
                            label = { Text(f.name, modifier = Modifier.padding(start = (16 * depth).dp)) },
                            badge = { if (count > 0) Text(count.toString()) },
                            selected = f.id == s.folderId,
                            onClick = {
                                vm.openFolder(f.id)
                                scope.launch { drawer.close() }
                            },
                            modifier = Modifier.padding(horizontal = 12.dp),
                        )
                    }
                    HorizontalDivider(Modifier.padding(vertical = 8.dp))
                    NavigationDrawerItem(
                        label = { Text(stringResource(R.string.settings)) },
                        selected = false,
                        onClick = {
                            scope.launch { drawer.close() }
                            onSettings()
                        },
                        modifier = Modifier.padding(horizontal = 12.dp),
                    )
                    // Keeps the last folder clear of the gesture/navigation bar.
                    Spacer(Modifier.navigationBarsPadding().height(12.dp))
                }
            }
        },
    ) {
        BackHandler(enabled = s.selecting) { vm.clearSelection() }
        Scaffold(
            topBar = {
                if (s.selecting) {
                    SelectionBar(s.selected.size, vm, s.folders, s.folderId)
                } else TopAppBar(
                    navigationIcon = {
                        IconButton(onClick = { scope.launch { drawer.open() } }) {
                            Icon(Icons.Filled.Menu, contentDescription = stringResource(R.string.section_folders))
                        }
                    },
                    title = {
                        if (searchOpen) {
                            TextField(
                                value = searchField,
                                onValueChange = ::search,
                                placeholder = { Text(stringResource(R.string.search_folder), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                                singleLine = true,
                                colors = TextFieldDefaults.colors(
                                    focusedContainerColor = Color.Transparent,
                                    unfocusedContainerColor = Color.Transparent,
                                ),
                                modifier = Modifier.fillMaxWidth(),
                            )
                        } else {
                            Text(s.folder?.name ?: stringResource(R.string.folder_inbox))
                        }
                    },
                    actions = {
                        IconButton(onClick = {
                            if (searchOpen) search(TextFieldValue(""))
                            searchOpen = !searchOpen
                        }) {
                            Icon(
                                if (searchOpen) Icons.Filled.Close else Icons.Filled.Search,
                                contentDescription = stringResource(if (searchOpen) R.string.close else R.string.search_folder),
                            )
                        }
                        if (!searchOpen) SortMenu(s.sort, vm::setSort)
                    },
                )
            },
            floatingActionButton = {
                if (!s.selecting) {
                    FloatingActionButton(onClick = onCompose, modifier = Modifier.size(48.dp)) {
                        Icon(Icons.Filled.Edit, contentDescription = stringResource(R.string.compose))
                    }
                }
            },
        ) { padding ->
            Column(Modifier.fillMaxSize().padding(padding)) {
                // The search commands, ready to tap (as in Gmail, in English in every language).
                if (searchOpen && s.query.isBlank()) SearchCommands { search(TextFieldValue(it, TextRange(it.length))) }
                PullToRefreshBox(
                    isRefreshing = s.refreshing,
                    onRefresh = vm::refresh,
                    modifier = Modifier.fillMaxSize(),
                ) {
                    val rows = s.rows
                    when {
                        // (With Gmail tabs the list stays, so another tab can be picked.)
                        rows.isEmpty() && s.refreshing && !s.searching -> Centered(stringResource(R.string.list_loading))
                        rows.isEmpty() && !s.refreshing && !s.searching && !s.tabs -> Centered(stringResource(R.string.list_empty))
                        else -> LazyColumn(Modifier.fillMaxSize()) {
                            // Gmail inbox tabs.
                            if (s.tabs && s.query.isBlank()) {
                                item(key = "tabs") { CategoryTabs(s.category, s.unreadByCategory, vm::setCategory) }
                            }
                            if (rows.isEmpty() && !s.refreshing) {
                                item(key = "empty") {
                                    Text(
                                        stringResource(R.string.list_empty),
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.padding(24.dp),
                                    )
                                }
                            }
                            items(rows, key = { it.latest.uid.toLong() }) { c ->
                                // Swipe a row: towards the start (left) archives it,
                                // towards the end (right) replies to it.
                                // Half the row: a slanted touch while scrolling the list
                                // must not delete or reply.
                                val swipe = rememberSwipeToDismissBoxState(positionalThreshold = { width -> width * 0.5f })
                                val swipeScope = rememberCoroutineScope()
                                // How far the finger really moved: a quick flick
                                // would otherwise count as a full swipe.
                                var dragged by remember { mutableFloatStateOf(0f) }
                                var rowWidth by remember { mutableIntStateOf(0) }
                                SwipeToDismissBox(
                                    state = swipe,
                                    modifier = Modifier
                                        .onSizeChanged { rowWidth = it.width }
                                        .pointerInput(Unit) {
                                            awaitEachGesture {
                                                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                                                dragged = 0f
                                                while (true) {
                                                    val change = awaitPointerEvent(PointerEventPass.Initial).changes
                                                        .firstOrNull { it.id == down.id } ?: break
                                                    dragged = change.position.x - down.position.x
                                                    if (!change.pressed) break
                                                }
                                            }
                                        },
                                    enableDismissFromStartToEnd = !s.selecting,
                                    // Delete; not in the Trash, where it would be for good without asking.
                                    enableDismissFromEndToStart = !s.selecting && !vm.deletesForever,
                                    backgroundContent = { SwipeBackground(swipe.dismissDirection) },
                                    onDismiss = { value ->
                                        // Less than half the row: put it back, do nothing.
                                        if (abs(dragged) < rowWidth * 0.5f) {
                                            swipeScope.launch { swipe.reset() }
                                            return@SwipeToDismissBox
                                        }
                                        when (value) {
                                            SwipeToDismissBoxValue.EndToStart -> vm.deleteRow(c.uids)
                                            SwipeToDismissBoxValue.StartToEnd -> {
                                                swipeScope.launch { swipe.reset() }
                                                onReply(s.folderId, c.latest.uid)
                                            }
                                            SwipeToDismissBoxValue.Settled -> {}
                                        }
                                    },
                                ) {
                                // Opaque, so the swipe background shows only beside the row.
                                Box(Modifier.background(MaterialTheme.colorScheme.background)) {
                                MessageRow(
                                    c.latest,
                                    count = c.uids.size,
                                    unread = c.unread > 0u,
                                    checked = if (s.selecting) s.selected.containsAll(c.uids) else null,
                                    onLongClick = { vm.toggle(c.uids) },
                                    starred = rowStarred(s, c.uids),
                                    // Gmail labels by their folder's name (not the open folder).
                                    labels = c.latest.labels.filter { it != s.folderId }.mapNotNull { id -> s.folders.firstOrNull { it.id == id }?.name },
                                    onStar = { on -> vm.toggleStar(c.uids, on) },
                                ) {
                                    if (s.selecting) {
                                        vm.toggle(c.uids)
                                    } else if (c.uids.size > 1) {
                                        onOpenConversation(s.folderId, c.uids)
                                    } else {
                                        // Opening reads it: update the dot right away.
                                        vm.opened(c.latest.uid)
                                        onOpen(s.folderId, c.latest.uid, rowStarred(s, c.uids))
                                    }
                                }
                                }
                                }
                                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                            }
                            // Room for the compose button over the last row.
                            item { Spacer(Modifier.height(88.dp)) }
                        }
                    }
                }
            }
        }
    }
}

/** Top bar while messages are checked: count, select menu and bulk actions. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SelectionBar(count: Int, vm: MailboxViewModel, folders: List<Folder>, current: String) {
    var menu by remember { mutableStateOf(false) }
    var moving by remember { mutableStateOf(false) }
    var labeling by remember { mutableStateOf(false) }
    if (labeling) {
        val s by vm.state.collectAsStateWithLifecycle()
        LabelDialog(vm.labelChoices(), s, onToggle = vm::toggleLabel, onDone = { labeling = false })
    }
    if (moving) {
        MoveDialog(folders.filter { it.selectable && it.id != current }, onDismiss = { moving = false }) { target ->
            moving = false
            vm.moveSelected(target)
        }
    }
    TopAppBar(
        navigationIcon = {
            IconButton(onClick = vm::clearSelection) {
                Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.select_none))
            }
        },
        title = { Text(fill(stringResource(R.string.selected_count), "n" to count)) },
        actions = {
            IconButton(onClick = { vm.deleteSelected() }) {
                Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.delete))
            }
            IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, contentDescription = null) }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                val item = @Composable { label: Int, action: () -> Unit ->
                    DropdownMenuItem(text = { Text(stringResource(label)) }, onClick = { menu = false; action() })
                }
                // "Select" heading over the choices.
                DropdownMenuItem(text = { Text(stringResource(R.string.select)) }, onClick = {}, enabled = false)
                item(R.string.select_all) { vm.select(MailboxViewModel.Pick.ALL) }
                item(R.string.select_read) { vm.select(MailboxViewModel.Pick.READ) }
                item(R.string.select_unread) { vm.select(MailboxViewModel.Pick.UNREAD) }
                HorizontalDivider()
                item(R.string.mark_read) { vm.markSelected(true) }
                item(R.string.mark_unread) { vm.markSelected(false) }
                item(R.string.star_add) { vm.starSelected(true) }
                item(R.string.star_remove) { vm.starSelected(false) }
                item(R.string.archive) { vm.archiveSelected() }
                item(if (vm.inSpam) R.string.spam_not else R.string.spam_report) { vm.spamSelected() }
                if (vm.gmail) item(R.string.labels) { labeling = true }
                item(R.string.move_to) { moving = true }
            }
        },
    )
}

/**
 * One list row: a message, or the newest message of a conversation of
 * [count]. A long press starts selection; [checked] is null outside it.
 */
@Composable
internal fun MessageRow(
    h: Header,
    count: Int = 1,
    unread: Boolean = !h.seen,
    checked: Boolean? = null,
    onLongClick: (() -> Unit)? = null,
    starred: Boolean = h.flagged,
    labels: List<String> = emptyList(),
    /** Shows a star that toggles (with the new state); null = no star. */
    onStar: ((Boolean) -> Unit)? = null,
    onClick: () -> Unit,
) {
    val weight = if (!unread) FontWeight.Normal else FontWeight.SemiBold
    Row(
        Modifier
            .fillMaxWidth()
            .background(if (checked == true) MaterialTheme.colorScheme.primaryContainer else Color.Transparent)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick)
            .padding(start = if (checked != null) 4.dp else 0.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (checked != null) Checkbox(checked = checked, onCheckedChange = { onClick() })
        Box(Modifier.weight(1f)) { RowBody(h, count, unread, weight, labels) }
        if (onStar != null) StarButton(starred) { onStar(!starred) }
    }
}

/** The star of a row: gold when on, faint when off. */
@Composable
fun StarButton(starred: Boolean, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(
            Icons.Filled.Star,
            contentDescription = stringResource(if (starred) R.string.star_remove else R.string.star_add),
            tint = if (starred) Color(0xFFE2A400) else MaterialTheme.colorScheme.outlineVariant,
        )
    }
}

/** Gmail labels for the checked messages: a check box per label. */
@Composable
private fun LabelDialog(choices: List<Folder>, s: MailboxState, onToggle: (String, Boolean) -> Unit, onDone: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDone,
        title = { Text(stringResource(R.string.labels)) },
        text = {
            if (choices.isEmpty()) {
                Text(stringResource(R.string.labels_none))
            } else {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    choices.forEach { f ->
                        // Checked when every checked message has the label.
                        val on = s.selected.isNotEmpty() &&
                            s.selected.all { uid -> s.shown.any { it.uid == uid && f.id in it.labels } }
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier.fillMaxWidth().clickable { onToggle(f.id, !on) },
                        ) {
                            Checkbox(checked = on, onCheckedChange = { onToggle(f.id, it) })
                            Text(f.name)
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDone) { Text(stringResource(R.string.close)) } },
    )
}

/** Gmail inbox tabs: Primary, Promotions, Social, Updates, Forums. */
@Composable
private fun CategoryTabs(current: Int, unread: Map<Int, Int>, onPick: (Int) -> Unit) {
    val labels = listOf(
        R.string.gmail_primary,
        R.string.gmail_promotions,
        R.string.gmail_social,
        R.string.gmail_updates,
        R.string.gmail_forums,
    )
    PrimaryScrollableTabRow(selectedTabIndex = current, edgePadding = 8.dp) {
        labels.forEachIndexed { i, label ->
            // Unread messages in each tab, as on the desktop: new mail in
            // another tab is visible without opening it.
            val count = unread[i] ?: 0
            Tab(
                selected = i == current,
                onClick = { onPick(i) },
                text = {
                    // The count sits next to the name (as on the desktop), not over it.
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(stringResource(label))
                        if (count > 0) Badge { Text(count.toString()) }
                    }
                },
            )
        }
    }
}

/** "Sort by…": the orders, the current one checked. */
@Composable
private fun SortMenu(current: SortOrder, onPick: (SortOrder) -> Unit) {
    var open by remember { mutableStateOf(false) }
    IconButton(onClick = { open = true }) {
        Icon(Icons.AutoMirrored.Filled.List, contentDescription = stringResource(R.string.sort_by))
    }
    DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
        DropdownMenuItem(text = { Text(stringResource(R.string.sort_by)) }, onClick = {}, enabled = false)
        SortOrder.entries.forEach { order ->
            DropdownMenuItem(
                text = { Text(stringResource(sortLabel(order))) },
                trailingIcon = { if (order == current) Icon(Icons.Filled.Check, contentDescription = null) },
                onClick = {
                    open = false
                    onPick(order)
                },
            )
        }
    }
}

/** The phone-language label of a sort order. */
private fun sortLabel(order: SortOrder): Int = when (order) {
    SortOrder.NEWEST -> R.string.sort_newest
    SortOrder.OLDEST -> R.string.sort_oldest
    SortOrder.LARGEST -> R.string.sort_largest
    SortOrder.SMALLEST -> R.string.sort_smallest
    SortOrder.SENDER_AZ -> R.string.sort_sender_az
    SortOrder.SENDER_ZA -> R.string.sort_sender_za
    SortOrder.SUBJECT_AZ -> R.string.sort_subject_az
}

/** Whether any message of a row is starred. */
private fun rowStarred(s: MailboxState, uids: List<UInt>): Boolean = s.shown.any { it.flagged && it.uid in uids }

@Composable
private fun RowBody(h: Header, count: Int, unread: Boolean, weight: FontWeight, labels: List<String>) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (unread) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
            }
            Text(h.from, fontWeight = weight, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            if (count > 1) {
                Text(
                    count.toString(),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(horizontal = 7.dp, vertical = 1.dp),
                )
            }
            Text(
                DateUtils.getRelativeTimeSpanString(h.timestamp * 1000).toString(),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Text(h.subject, fontWeight = weight, maxLines = 1, overflow = TextOverflow.Ellipsis)
        // Gmail labels as small chips.
        if (labels.isNotEmpty()) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(vertical = 2.dp)) {
                labels.take(3).forEach { label ->
                    Text(
                        label,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        modifier = Modifier
                            .clip(CircleShape)
                            .background(MaterialTheme.colorScheme.surfaceVariant)
                            .padding(horizontal = 8.dp, vertical = 2.dp),
                    )
                }
            }
        }
        Text(
            h.preview,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** "Move to": the folders, as on the drawer. */
@Composable
private fun MoveDialog(folders: List<Folder>, onDismiss: () -> Unit, onPick: (String) -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.move_to)) },
        text = {
            LazyColumn {
                items(folders, key = { it.id }) { f ->
                    Text(
                        f.name,
                        modifier = Modifier
                            .fillMaxWidth()
                            .combinedClickable(onClick = { onPick(f.id) })
                            .padding(start = (16 * (f.path.size - 1).coerceAtLeast(0)).dp, top = 12.dp, bottom = 12.dp),
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun ConfirmBulkDelete(vm: MailboxViewModel) {
    AlertDialog(
        onDismissRequest = vm::clearSelection,
        title = { Text(stringResource(R.string.confirm_delete_title)) },
        text = { Text(stringResource(R.string.cannot_undo)) },
        confirmButton = { TextButton(onClick = { vm.deleteSelected(confirmed = true) }) { Text(stringResource(R.string.delete_forever)) } },
        dismissButton = { TextButton(onClick = vm::clearSelection) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun Centered(text: String, color: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) { Text(text, color = color) }
}

/** What a swipe does, under the row: reply at the start, delete at the end. */
@Composable
private fun SwipeBackground(direction: SwipeToDismissBoxValue) {
    if (direction == SwipeToDismissBoxValue.Settled) return
    val reply = direction == SwipeToDismissBoxValue.StartToEnd
    val colors = MaterialTheme.colorScheme
    Box(
        Modifier
            .fillMaxSize()
            .background(if (reply) colors.primaryContainer else colors.errorContainer)
            .padding(horizontal = 24.dp),
        contentAlignment = if (reply) Alignment.CenterStart else Alignment.CenterEnd,
    ) {
        if (reply) {
            Icon(painterResource(R.drawable.ic_reply), contentDescription = stringResource(R.string.reply))
        } else {
            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.delete), tint = colors.onErrorContainer)
        }
    }
}

/** Drawer order of a folder kind (stable sort keeps the rest alphabetical). */
private fun kindRank(kind: FolderKind): Int = when (kind) {
    FolderKind.INBOX -> 0
    FolderKind.SENT -> 1
    FolderKind.DRAFTS -> 2
    FolderKind.ARCHIVE -> 3
    FolderKind.SPAM -> 4
    FolderKind.TRASH -> 5
    FolderKind.OTHER -> 6
}

/** Ferrowl Pro: more than one account, yearly subscription through Google Play. */
@Composable
private fun ProOffer(price: String?, vm: MailboxViewModel) {
    val activity = LocalActivity.current
    val billing = LocalBilling.current
    AlertDialog(
        onDismissRequest = vm::dismissPro,
        title = { Text(stringResource(R.string.pro_title)) },
        text = { Text(fill(stringResource(R.string.pro_text), "price" to (price ?: "…"))) },
        confirmButton = {
            TextButton(onClick = {
                if (activity == null || !billing.buy(activity)) vm.proUnavailable() else vm.purchaseStarted()
            }) { Text(stringResource(R.string.pro_subscribe)) }
        },
        dismissButton = { TextButton(onClick = vm::dismissPro) { Text(stringResource(R.string.pro_not_now)) } },
    )
}

/** Chips that start a search with a command (`from:`, `has:attachment`, `is:unread`, `is:starred`). */
@Composable
private fun SearchCommands(onPick: (String) -> Unit) {
    val commands = listOf(
        "from:" to R.string.from,
        "has:attachment " to R.string.filter_attachments,
        "is:unread " to R.string.filter_unread,
        "is:starred " to R.string.filter_starred,
    )
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        commands.forEach { (command, label) ->
            SuggestionChip(
                onClick = { onPick(command) },
                label = { Text("${command.trim()} · ${stringResource(label)}") },
            )
        }
    }
}
