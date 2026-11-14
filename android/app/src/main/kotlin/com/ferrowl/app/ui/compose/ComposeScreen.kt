package com.ferrowl.app.ui.compose

import android.net.Uri
import android.text.format.Formatter
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.ferrowl.app.R
import com.ferrowl.app.ui.message.ReplyKind
import com.ferrowl.app.ui.notice.ReportError
import com.ferrowl.core.AddressSuggestion
import java.io.File

/**
 * Writing a message: new, reply, reply all or forward, laid out like the
 * desktop window (From / To / Subject rows with their label on the side,
 * Cc and Bcc on request, a formatting bar, Send at the bottom). Markdown
 * typed in the body (bold, lists, links) is also sent as HTML.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ComposeScreen(
    kind: ReplyKind?,
    folderId: String,
    uid: UInt,
    to: String,
    onClose: () -> Unit,
    cc: String = "",
    subject: String = "",
    body: String = "",
    files: List<String> = emptyList(),
    vm: ComposeViewModel = hiltViewModel(),
) {
    LaunchedEffect(Unit) {
        vm.start(kind, folderId, uid, to, cc, subject, body)
        // Files shared from another app, attached once.
        if (files.isNotEmpty()) vm.addShared(files.map(Uri::parse))
    }
    val s by vm.state.collectAsStateWithLifecycle()
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris -> vm.addFiles(uris) }
    // Photos and videos from the gallery (system picker: no permission needed).
    val pickMedia = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia()) { uris -> vm.addFiles(uris) }
    // A photo from the camera app, written to a file of ours (FileProvider).
    val context = LocalContext.current
    var photo by rememberSaveable { mutableStateOf<String?>(null) }
    val shoot = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        val path = photo
        photo = null
        if (path != null) {
            val file = File(path)
            if (ok && file.length() > 0) {
                vm.addFiles(listOf(FileProvider.getUriForFile(context, "${context.packageName}.files", file)))
            } else {
                file.delete()
            }
        }
    }
    var attachMenu by remember { mutableStateOf(false) }
    ReportError(s.error, vm::errorShown)
    LaunchedEffect(s.sent) { if (s.sent) onClose() }
    var askDiscard by remember { mutableStateOf(false) }
    val leave = { if (s.edited) askDiscard = true else onClose() }
    BackHandler(onBack = leave)
    var showCc by rememberSaveable { mutableStateOf(false) }
    // The body keeps its cursor and selection here (the formatting bar
    // wraps the selection); the text goes to the view model.
    var body by rememberSaveable(stateSaver = TextFieldValue.Saver) { mutableStateOf(TextFieldValue("")) }
    // The view model fills the body once: from a reply/forward draft (after
    // loading) or at once from a link, a share or mailto (loading stays
    // false then, so the body itself must restart this). Once filled, the
    // user's edits are the text, even when they empty it.
    var filled by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(s.loading, s.body) {
        if (!filled && !s.loading && body.text.isEmpty() && s.body.isNotEmpty()) {
            body = TextFieldValue(s.body)
            filled = true
        }
    }
    fun edit(v: TextFieldValue) {
        body = v
        vm.setBody(v.text)
    }

    val title = when (kind) {
        null -> R.string.compose_new
        ReplyKind.FORWARD -> R.string.compose_forward
        else -> R.string.compose_reply
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(title)) },
                navigationIcon = {
                    IconButton(onClick = leave) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.discard)) }
                },
            )
        },
        bottomBar = {
            Surface(color = MaterialTheme.colorScheme.background) {
                Column {
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                Row(
                    Modifier.fillMaxWidth().navigationBarsPadding().imePadding().padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Button(onClick = vm::send, enabled = !s.loading && !s.sending) {
                        if (s.sending) {
                            CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onPrimary)
                        } else {
                            Icon(Icons.AutoMirrored.Filled.Send, contentDescription = null, modifier = Modifier.size(18.dp))
                        }
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.send))
                    }
                    Box {
                        IconButton(onClick = { attachMenu = true }, enabled = !s.sending) {
                            Icon(painterResource(R.drawable.ic_attach), contentDescription = stringResource(R.string.attach))
                        }
                        DropdownMenu(expanded = attachMenu, onDismissRequest = { attachMenu = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.attach_file)) }, onClick = {
                                attachMenu = false
                                pick.launch(arrayOf("*/*"))
                            })
                            DropdownMenuItem(text = { Text(stringResource(R.string.attach_photo)) }, onClick = {
                                attachMenu = false
                                pickMedia.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo))
                            })
                            DropdownMenuItem(text = { Text(stringResource(R.string.attach_camera)) }, onClick = {
                                attachMenu = false
                                val dir = File(context.cacheDir, "camera").apply { mkdirs() }
                                val file = File(dir, "IMG_${System.currentTimeMillis()}.jpg")
                                photo = file.path
                                shoot.launch(FileProvider.getUriForFile(context, "${context.packageName}.files", file))
                            })
                        }
                    }
                    Spacer(Modifier.weight(1f))
                    IconButton(onClick = leave) {
                        Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.discard))
                    }
                }
                }
            }
        },
    ) { padding ->
        Column(
            Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState()),
        ) {
            if (s.from.isNotEmpty()) {
                FieldRow(stringResource(R.string.from)) {
                    Text(s.from, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
            FieldRow(
                stringResource(R.string.to),
                trailing = {
                    if (!showCc && s.cc.isEmpty() && s.bcc.isEmpty()) {
                        TextButton(onClick = { showCc = true }) {
                            Text("${stringResource(R.string.cc)} ${stringResource(R.string.bcc)}", color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                },
            ) { Input(s.to, vm::setTo, KeyboardType.Email) }
            if (s.suggestFor == true) Suggestions(s.suggestions, vm::pick)
            if (showCc || s.cc.isNotEmpty() || s.bcc.isNotEmpty()) {
                FieldRow(stringResource(R.string.cc)) { Input(s.cc, vm::setCc, KeyboardType.Email) }
                if (s.suggestFor == false) Suggestions(s.suggestions, vm::pick)
                FieldRow(stringResource(R.string.bcc)) { Input(s.bcc, vm::setBcc, KeyboardType.Email) }
            }
            FieldRow(stringResource(R.string.subject)) {
                Input(s.subject, vm::setSubject, bold = true)
            }
            if (s.attachments.isNotEmpty()) {
                FlowRow(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    s.attachments.forEach { a -> AttachmentChip(a) { vm.removeAttachment(a.path) } }
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            }
            FormatBar(body, ::edit)
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            BasicTextField(
                value = body,
                onValueChange = ::edit,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                modifier = Modifier.fillMaxWidth().heightIn(min = 240.dp).padding(16.dp),
                decorationBox = { inner ->
                    if (body.text.isEmpty()) {
                        Text(stringResource(R.string.body_placeholder), style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    inner()
                },
            )
        }
    }

    if (askDiscard) {
        AlertDialog(
            onDismissRequest = { askDiscard = false },
            title = { Text(stringResource(R.string.discard)) },
            confirmButton = { TextButton(onClick = { askDiscard = false; onClose() }) { Text(stringResource(R.string.discard)) } },
            dismissButton = { TextButton(onClick = { askDiscard = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** One header row: the label on the side, the value, an optional action at the end. */
@Composable
private fun FieldRow(label: String, trailing: @Composable () -> Unit = {}, content: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(start = 16.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(72.dp))
        // A little room on the end side, so values do not touch the edge.
        Box(Modifier.weight(1f).padding(end = 8.dp)) { content() }
        trailing()
    }
    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
}

/** A borderless one-line field, as in the desktop form. */
@Composable
private fun Input(value: String, onChange: (String) -> Unit, keyboard: KeyboardType = KeyboardType.Text, bold: Boolean = false) {
    BasicTextField(
        value = value,
        onValueChange = onChange,
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = keyboard),
        textStyle = MaterialTheme.typography.bodyLarge.copy(
            color = MaterialTheme.colorScheme.onSurface,
            fontWeight = if (bold) FontWeight.SemiBold else FontWeight.Normal,
        ),
        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
        modifier = Modifier.fillMaxWidth().padding(vertical = 14.dp),
    )
}

/** Bold, italic, strikethrough, list, quote and link: Markdown around the selection. */
@Composable
private fun FormatBar(body: TextFieldValue, onChange: (TextFieldValue) -> Unit) {
    Row(
        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphButton("B", FontWeight.Bold, null, R.string.fmt_bold) { onChange(wrap(body, "**")) }
        GlyphButton("I", FontWeight.Normal, FontStyle.Italic, R.string.fmt_italic) { onChange(wrap(body, "_")) }
        GlyphButton("S", FontWeight.Normal, null, R.string.fmt_strike, strike = true) { onChange(wrap(body, "~~")) }
        IconFormat(R.drawable.ic_format_list, R.string.fmt_bullets) { onChange(prefixLine(body, "- ")) }
        IconFormat(R.drawable.ic_format_quote, R.string.fmt_quote) { onChange(prefixLine(body, "> ")) }
        IconFormat(R.drawable.ic_link, R.string.fmt_link) { onChange(link(body)) }
    }
}

@Composable
private fun GlyphButton(
    glyph: String,
    weight: FontWeight,
    style: FontStyle?,
    description: Int,
    strike: Boolean = false,
    onClick: () -> Unit,
) {
    val label = stringResource(description).substringBefore(" (")
    IconButton(onClick = onClick, modifier = Modifier.semantics { contentDescription = label }) {
        Text(
            glyph,
            fontWeight = weight,
            fontStyle = style,
            textDecoration = if (strike) TextDecoration.LineThrough else null,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun IconFormat(icon: Int, description: Int, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(
            painterResource(icon),
            contentDescription = stringResource(description).substringBefore(" ("),
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

/** `mark` around the selection (or an empty pair at the cursor). */
private fun wrap(v: TextFieldValue, mark: String): TextFieldValue {
    val sel = v.selection
    val text = v.text.substring(0, sel.min) + mark + v.text.substring(sel.min, sel.max) + mark + v.text.substring(sel.max)
    return TextFieldValue(text, TextRange(sel.min + mark.length, sel.max + mark.length))
}

/** `prefix` at the start of the line with the cursor. */
private fun prefixLine(v: TextFieldValue, prefix: String): TextFieldValue {
    val start = v.text.lastIndexOf('\n', (v.selection.min - 1).coerceAtLeast(0)).let { if (it < 0 || v.selection.min == 0) 0 else it + 1 }
    val text = v.text.substring(0, start) + prefix + v.text.substring(start)
    return TextFieldValue(text, TextRange(v.selection.min + prefix.length, v.selection.max + prefix.length))
}

/** A Markdown link around the selection: [text](https://). */
private fun link(v: TextFieldValue): TextFieldValue {
    val sel = v.selection
    val label = v.text.substring(sel.min, sel.max)
    val inserted = "[$label](https://)"
    val text = v.text.substring(0, sel.min) + inserted + v.text.substring(sel.max)
    val urlEnd = sel.min + inserted.length - 1
    return TextFieldValue(text, TextRange(urlEnd))
}

/** Addresses matching the word being typed; tap one to insert it. */
@Composable
private fun Suggestions(list: List<AddressSuggestion>, onPick: (AddressSuggestion) -> Unit) {
    Column(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.surfaceContainer)) {
        list.forEach { s ->
            Column(
                Modifier
                    .fillMaxWidth()
                    .clickable { onPick(s) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
            ) {
                if (s.name.isNotBlank()) Text(s.name, style = MaterialTheme.typography.bodyMedium)
                Text(s.address, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

/** An attached file as a chip: name, size and a button to remove it. */
@Composable
private fun AttachmentChip(a: Attachment, onRemove: () -> Unit) {
    val context = LocalContext.current
    Surface(
        shape = RoundedCornerShape(10.dp),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(Modifier.padding(start = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(painterResource(R.drawable.ic_attach), contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(6.dp))
            Column(Modifier.widthIn(max = 180.dp)) {
                Text(a.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(Formatter.formatShortFileSize(context, a.size), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            IconButton(onClick = onRemove, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.remove_attachment), modifier = Modifier.size(18.dp))
            }
        }
    }
}
