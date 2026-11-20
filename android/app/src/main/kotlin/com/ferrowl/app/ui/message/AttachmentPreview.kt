package com.ferrowl.app.ui.message

import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color as AndroidColor
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import android.text.format.Formatter
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.FileProvider
import com.ferrowl.app.R
import com.ferrowl.app.ui.notice.LocalNotices
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

/** PDF pages shown at most; the rest through "Open with…" (as on desktop). */
private const val MAX_PDF_PAGES = 50

/** Text shown at most (characters). */
private const val MAX_TEXT = 200_000

/** What the preview can show of a file, by its extension. */
internal enum class PreviewKind { PDF, IMAGE, TEXT, NONE }

internal fun previewKind(name: String): PreviewKind = when (name.substringAfterLast('.', "").lowercase()) {
    "pdf" -> PreviewKind.PDF
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "heif" -> PreviewKind.IMAGE
    "txt", "csv", "md", "log", "json", "xml", "ics", "vcf", "eml", "asc" -> PreviewKind.TEXT
    else -> PreviewKind.NONE
}

/**
 * An attachment shown full screen inside the app, as Quick Look does on
 * iOS: PDF pages, pictures (pinch to zoom) and text. Share and "Open
 * with…" pass it on; files it cannot show offer "Open with…" at once.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AttachmentPreview(file: File, onClose: () -> Unit) {
    val context = LocalContext.current
    val notices = LocalNotices.current
    val noPreview = stringResource(R.string.preview_failed)
    val openWith = { if (!openAttachment(context, file)) notices.error(noPreview) }
    var menu by remember { mutableStateOf(false) }
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(file.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = {
                        IconButton(onClick = onClose) { Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.close)) }
                    },
                    actions = {
                        IconButton(onClick = { shareAttachment(context, file) }) {
                            Icon(Icons.Filled.Share, contentDescription = stringResource(R.string.share))
                        }
                        IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, contentDescription = null) }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.open_with)) }, onClick = { menu = false; openWith() })
                        }
                    },
                )
            },
        ) { padding ->
            Box(Modifier.fillMaxSize().padding(padding)) {
                when (previewKind(file.name)) {
                    PreviewKind.PDF -> PdfPages(file, onFailed = { NoPreview(file, openWith) })
                    PreviewKind.IMAGE -> Picture(file, onFailed = { NoPreview(file, openWith) })
                    PreviewKind.TEXT -> TextFile(file)
                    PreviewKind.NONE -> NoPreview(file, openWith)
                }
            }
        }
    }
}

@Composable
private fun NoPreview(file: File, openWith: () -> Unit) {
    val context = LocalContext.current
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterVertically),
    ) {
        Text(file.name, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
        Text(
            Formatter.formatShortFileSize(context, file.length()),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(stringResource(R.string.preview_failed), style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
        TextButton(onClick = openWith) { Text(stringResource(R.string.open_with)) }
    }
}

/** The pages one under the other, drawn as the list scrolls to them. */
@Composable
private fun PdfPages(file: File, onFailed: @Composable () -> Unit) {
    // PdfRenderer draws one page at a time: the pages take turns.
    val lock = remember(file) { Mutex() }
    val doc = remember(file) {
        runCatching {
            val fd = ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
            fd to PdfRenderer(fd)
        }.getOrNull()
    }
    DisposableEffect(doc) {
        onDispose {
            doc?.let { (fd, renderer) ->
                // After any page still being drawn.
                kotlinx.coroutines.runBlocking { lock.withLock { renderer.close(); fd.close() } }
            }
        }
    }
    if (doc == null || doc.second.pageCount == 0) {
        onFailed()
        return
    }
    val renderer = doc.second
    val total = renderer.pageCount
    val shown = minOf(total, MAX_PDF_PAGES)
    val width = with(LocalDensity.current) { LocalConfiguration.current.screenWidthDp.dp.roundToPx() }
    val ratios = remember(file) { mutableStateMapOf<Int, Float>() }
    LazyColumn(
        Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(shown) { i ->
            val page by produceState<Bitmap?>(null, file, i) {
                value = withContext(Dispatchers.IO) {
                    lock.withLock {
                        runCatching {
                            renderer.openPage(i).use { p ->
                                ratios[i] = p.width.toFloat() / p.height
                                val height = (width.toLong() * p.height / p.width).toInt().coerceAtLeast(1)
                                Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).also {
                                    it.eraseColor(AndroidColor.WHITE)
                                    p.render(it, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                                }
                            }
                        }.getOrNull()
                    }
                }
            }
            val ratio = ratios[i] ?: (1 / 1.414f)
            Box(Modifier.fillMaxWidth().aspectRatio(ratio).background(Color.White), contentAlignment = Alignment.Center) {
                page?.let { Image(it.asImageBitmap(), contentDescription = null, modifier = Modifier.fillMaxSize()) }
                    ?: CircularProgressIndicator()
            }
        }
        if (total > shown) {
            item {
                Text(
                    stringResource(R.string.pdf_more_pages).replace("{n}", (total - shown).toString()),
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(16.dp),
                )
            }
        }
    }
}

/** A picture fitted to the screen; pinch to zoom, drag to move. */
@Composable
private fun Picture(file: File, onFailed: @Composable () -> Unit) {
    val maxSide = with(LocalDensity.current) {
        maxOf(LocalConfiguration.current.screenWidthDp, LocalConfiguration.current.screenHeightDp).dp.roundToPx() * 2
    }
    val bitmap by produceState<Result<Bitmap>?>(null, file) {
        value = withContext(Dispatchers.IO) { runCatching { decodeScaled(file, maxSide) ?: error("not a picture") } }
    }
    var scale by remember { mutableFloatStateOf(1f) }
    var dx by remember { mutableFloatStateOf(0f) }
    var dy by remember { mutableFloatStateOf(0f) }
    val zoom = rememberTransformableState { change, pan, _ ->
        scale = (scale * change).coerceIn(1f, 6f)
        if (scale == 1f) { dx = 0f; dy = 0f } else { dx += pan.x; dy += pan.y }
    }
    when (val b = bitmap) {
        null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        else -> b.getOrNull()?.let {
            Image(
                it.asImageBitmap(),
                contentDescription = file.name,
                modifier = Modifier.fillMaxSize().transformable(zoom).graphicsLayer {
                    scaleX = scale; scaleY = scale; translationX = dx; translationY = dy
                },
            )
        } ?: onFailed()
    }
}

/** Decodes a picture no larger than [maxSide] pixels on its longer side. */
private fun decodeScaled(file: File, maxSide: Int): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.path, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= maxSide) sample *= 2
    return BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
}

@Composable
private fun TextFile(file: File) {
    val text by produceState<Pair<String, Boolean>?>(null, file) {
        value = withContext(Dispatchers.IO) {
            val all = runCatching { file.readText() }.getOrDefault("")
            all.take(MAX_TEXT) to (all.length > MAX_TEXT)
        }
    }
    val (body, cut) = text ?: return
    SelectionContainer {
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
            Text(
                body,
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                modifier = Modifier.horizontalScroll(rememberScrollState()),
            )
            if (cut) {
                Text(
                    stringResource(R.string.preview_cut),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 12.dp),
                )
            }
        }
    }
}

/** The system's share sheet for the file (mail, chat, Drive, Files…). */
private fun shareAttachment(context: Context, file: File) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val send = Intent(Intent.ACTION_SEND)
        .setType(mimeOf(file.name))
        .putExtra(Intent.EXTRA_STREAM, uri)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    runCatching { context.startActivity(Intent.createChooser(send, file.name)) }
}
