package com.ferrowl.app.ui.message

import android.content.Intent
import android.util.Patterns
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FabPosition
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.net.toUri
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.R
import com.ferrowl.app.ui.mailbox.StarButton
import com.ferrowl.app.ui.notice.LocalNotices
import com.ferrowl.app.ui.theme.LocalRemoteImages
import com.ferrowl.core.Message

/** What the user can do with an open message. */
enum class ReplyKind { REPLY, REPLY_ALL, FORWARD }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MessageScreen(
    folderId: String,
    uid: UInt,
    starred: Boolean,
    onBack: () -> Unit,
    onReply: (ReplyKind) -> Unit,
    /** Deleted, archived or moved: back to the message list (also from a conversation). */
    onGone: () -> Unit = onBack,
    /**
     * To the Trash with "Moved to Trash · Undo" (the list does it, so Undo
     * puts the row back); null: moved at once.
     */
    onTrash: (() -> Unit)? = null,
    vm: MessageViewModel = hiltViewModel(),
) {
    LaunchedEffect(folderId, uid) { vm.load(folderId, uid, starred) }
    val star by vm.starred.collectAsStateWithLifecycle()
    val state by vm.state.collectAsStateWithLifecycle()
    LaunchedEffect(state) { if (state == MessageState.Gone) onGone() }
    val invite by vm.invite.collectAsStateWithLifecycle()
    var menu by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {},
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back_to_mail))
                    }
                },
                actions = {
                    if (state is MessageState.Shown) {
                        StarButton(star, onClick = vm::toggleStar)
                        IconButton(onClick = {
                            when {
                                vm.deletesForever -> confirmDelete = true
                                onTrash != null -> onTrash()
                                else -> vm.delete()
                            }
                        }) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.delete))
                        }
                        IconButton(onClick = { menu = true }) {
                            Icon(Icons.Filled.MoreVert, contentDescription = null)
                        }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.archive)) }, onClick = { menu = false; vm.archive() })
                            DropdownMenuItem(text = { Text(stringResource(R.string.mark_unread)) }, onClick = { menu = false; vm.markUnread() })
                            DropdownMenuItem(
                                text = { Text(stringResource(if (vm.inSpam) R.string.spam_not else R.string.spam_report)) },
                                onClick = { menu = false; vm.spam() },
                            )
                            if (!vm.inSpam) {
                                DropdownMenuItem(text = { Text(stringResource(R.string.block_sender)) }, onClick = { menu = false; vm.blockSender() })
                            }
                            DropdownMenuItem(text = { Text(stringResource(R.string.reply_all)) }, onClick = { menu = false; onReply(ReplyKind.REPLY_ALL) })
                            DropdownMenuItem(text = { Text(stringResource(R.string.forward)) }, onClick = { menu = false; onReply(ReplyKind.FORWARD) })
                        }
                    }
                },
            )
        },
        floatingActionButton = {
            if (state is MessageState.Shown) {
                // Reply, reply all, forward: small, at the bottom left, on a
                // raised pill that stands out over the message (as on iOS).
                Surface(
                    // A little away from the edge (on top of the button's own margin).
                    modifier = Modifier.padding(start = 12.dp),
                    shape = RoundedCornerShape(50),
                    color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.94f),
                    tonalElevation = 3.dp,
                    shadowElevation = 4.dp,
                ) {
                    Row(Modifier.padding(horizontal = 4.dp)) {
                        IconButton(onClick = { onReply(ReplyKind.REPLY) }) {
                            Icon(painterResource(R.drawable.ic_reply), contentDescription = stringResource(R.string.reply), modifier = Modifier.size(24.dp))
                        }
                        IconButton(onClick = { onReply(ReplyKind.REPLY_ALL) }) {
                            Icon(painterResource(R.drawable.ic_reply_all), contentDescription = stringResource(R.string.reply_all), modifier = Modifier.size(24.dp))
                        }
                        IconButton(onClick = { onReply(ReplyKind.FORWARD) }) {
                            Icon(painterResource(R.drawable.ic_forward), contentDescription = stringResource(R.string.forward), modifier = Modifier.size(24.dp))
                        }
                    }
                }
            }
        },
        floatingActionButtonPosition = FabPosition.Start,
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (val s = state) {
                MessageState.Loading, MessageState.Gone -> CircularProgressIndicator(Modifier.align(Alignment.Center))
                // The reason is shown as a notice at the top; the page stays empty.
                is MessageState.Failed -> {
                    val notices = LocalNotices.current
                    LaunchedEffect(s.error) { notices.error(s.error) }
                }
                is MessageState.Shown -> Body(s.message) {
                    s.message.invitation?.let { inv ->
                        InvitationCard(inv, invite, onRespond = vm::respond, onRemove = vm::removeCancelled)
                    }
                    val insights by vm.insights.collectAsStateWithLifecycle()
                    InsightCards(insights, onAddDue = vm::addDue)
                }
            }
        }
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.confirm_delete_title)) },
            text = { Text(stringResource(R.string.cannot_undo)) },
            confirmButton = {
                TextButton(onClick = { confirmDelete = false; vm.delete() }) { Text(stringResource(R.string.delete_forever)) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun Body(m: Message, extra: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(m.subject, style = MaterialTheme.typography.titleLarge)
            Text("${m.from} <${m.fromAddress}>", style = MaterialTheme.typography.bodyMedium)
            if (m.to.isNotBlank()) {
                Text(
                    stringResource(R.string.to) + ": " + m.to,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        extra()
        val html = m.html
        if (html != null) {
            HtmlView(html)
        } else {
            SelectionContainer {
                Text(linked(m.text), modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp))
            }
        }
    }
}

/**
 * Plain text with its web and e-mail addresses made into links (opened by
 * the system: browser, or a new message for `mailto:`).
 */
internal fun linked(text: String): AnnotatedString = buildAnnotatedString {
    append(text)
    val style = TextLinkStyles(SpanStyle(textDecoration = TextDecoration.Underline))
    for (m in Patterns.WEB_URL.toRegex().findAll(text)) {
        val url = if (m.value.contains("://")) m.value else "https://${m.value}"
        // Bare words like "file.txt" also match WEB_URL: only real addresses.
        if (m.value.contains("://") || m.value.startsWith("www.", ignoreCase = true)) {
            addLink(LinkAnnotation.Url(url, style), m.range.first, m.range.last + 1)
        }
    }
    for (m in Patterns.EMAIL_ADDRESS.toRegex().findAll(text)) {
        addLink(LinkAnnotation.Url("mailto:${m.value}", style), m.range.first, m.range.last + 1)
    }
}

/**
 * HTML mail in a locked-down WebView: no JavaScript, no file or content
 * access, links open in the browser. Remote images load unless turned off
 * in the settings; then a bar offers to load them for this message.
 *
 * Fitting the screen: the page gets the phone's width (see [fitToScreen]);
 * layouts wider than that (fixed-width newsletters) are zoomed out to fit
 * by the overview mode, and can be pinched to zoom in.
 */
@Composable
private fun HtmlView(html: String) {
    var loadImages by remember(html) { mutableStateOf(false) }
    val remote = LocalRemoteImages.current || loadImages
    // Only when the mail has pictures from the network.
    val hasRemote = remember(html) { Regex("""<img[^>]+src\s*=\s*["']?https?:""", RegexOption.IGNORE_CASE).containsMatchIn(html) }
    if (!remote && hasRemote) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.images_blocked),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { loadImages = true }) { Text(stringResource(R.string.load_images)) }
        }
    }
    // Dark theme: plain mail follows it; mail with its own colors stays light.
    val colors = MaterialTheme.colorScheme
    val dark = colors.surface.luminance() < 0.5f
    val page = fitToScreen(html, if (dark) DarkPage(cssColor(colors.onSurface), cssColor(colors.primary)) else null)
    AndroidView(
        factory = { context ->
            WebView(context).apply {
                settings.javaScriptEnabled = false
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.domStorageEnabled = false
                settings.useWideViewPort = true
                settings.loadWithOverviewMode = true
                settings.setSupportZoom(true)
                settings.builtInZoomControls = true
                settings.displayZoomControls = false
                // The app's background shows through plain mail in dark mode.
                setBackgroundColor(android.graphics.Color.TRANSPARENT)
                webViewClient = object : WebViewClient() {
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                        val url = request.url
                        if (url.scheme in setOf("http", "https", "mailto")) {
                            context.startActivity(Intent(Intent.ACTION_VIEW, url.toString().toUri()))
                        }
                        return true
                    }
                }
            }
        },
        update = { view ->
            view.settings.blockNetworkImage = !remote
            // Load only when the message changes, not on every recomposition.
            if (view.tag != page) {
                view.tag = page
                view.loadDataWithBaseURL(null, page, "text/html", "UTF-8", null)
            }
        },
        modifier = Modifier.fillMaxSize(),
    )
}

/**
 * Adds what makes mail readable on a phone. The mail's own viewport is
 * replaced: many newsletters ask for "device width, scale 1" and then use a
 * fixed 600 px table, which would run off the screen. Ours keeps the device
 * width (so responsive layouts still adapt) without fixing the scale, so the
 * overview mode can zoom wider layouts out to fit. Images and media never
 * exceed the screen; long words and links wrap.
 */
internal fun fitToScreen(html: String, dark: DarkPage? = null): String {
    // Mail with colors of its own is written for a light page: it keeps one.
    val followsDark = dark != null && !hasOwnColors(html)
    val page = if (followsDark) "color-scheme:dark;background:transparent;color:${dark!!.text}" else "background:#fff;color:#1F1B18"
    val links = if (followsDark) "a{color:${dark!!.link}}" else ""
    val head = "<meta name=\"viewport\" content=\"width=device-width\">" +
        "<style>" +
        "html{$page}" +
        "body{margin:12px;overflow-wrap:anywhere}" +
        links +
        "img,video{max-width:100%!important;height:auto!important}" +
        "pre{white-space:pre-wrap}" +
        "</style>"
    val clean = html.replace(Regex("<meta[^>]*name\\s*=\\s*[\"']?viewport[^>]*>", RegexOption.IGNORE_CASE), "")
    val at = Regex("<head[^>]*>", RegexOption.IGNORE_CASE).find(clean)
    return if (at != null) {
        clean.substring(0, at.range.last + 1) + head + clean.substring(at.range.last + 1)
    } else {
        "<!doctype html><html><head>$head</head><body>$clean</body></html>"
    }
}

/** Text and link colors (CSS) for plain mail shown in the dark theme. */
internal data class DarkPage(val text: String, val link: String)

/**
 * Whether the mail sets colors or styles of its own (newsletters, receipts).
 * Plain mail (text with links, as most mail apps send it) follows the theme.
 */
internal fun hasOwnColors(html: String): Boolean =
    Regex("<style|bgcolor\\s*=|background|color\\s*[:=]", RegexOption.IGNORE_CASE).containsMatchIn(html)

private fun cssColor(c: Color): String = String.format("#%06X", c.toArgb() and 0xFFFFFF)
