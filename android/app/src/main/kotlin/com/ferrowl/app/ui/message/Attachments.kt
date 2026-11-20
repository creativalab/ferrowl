package com.ferrowl.app.ui.message

import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.text.format.Formatter
import android.webkit.MimeTypeMap
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.ferrowl.app.R
import com.ferrowl.app.ui.notice.LocalNotices
import com.ferrowl.core.Attachment
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * The message's attachments, side by side under the header (the HTML below
 * scrolls on its own). A tap opens one in an app that shows its kind of
 * file (gallery, PDF viewer…), which also offers to share or save it, as
 * Quick Look does on iOS.
 */
@Composable
internal fun AttachmentRow(uid: UInt, attachments: List<Attachment>) {
    if (attachments.isEmpty()) return
    val context = LocalContext.current
    val notices = LocalNotices.current
    val scope = rememberCoroutineScope()
    val noPreview = stringResource(R.string.preview_failed)
    val label = stringResource(R.string.preview_attachment)
    Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(
            if (attachments.size == 1) {
                stringResource(R.string.attachments_one)
            } else {
                stringResource(R.string.attachments_many).replace("{n}", attachments.size.toString())
            },
            style = MaterialTheme.typography.labelLarge,
        )
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            attachments.forEach { a ->
                Surface(
                    onClick = {
                        scope.launch {
                            val file = withContext(Dispatchers.IO) { attachmentFile(context, uid, a) }
                            if (file == null || !openAttachment(context, file)) notices.error(noPreview)
                        }
                    },
                    shape = RoundedCornerShape(10.dp),
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
                    color = MaterialTheme.colorScheme.surface,
                    modifier = Modifier.semantics { contentDescription = "$label: ${a.name}" },
                ) {
                    Row(Modifier.padding(horizontal = 10.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            painterResource(R.drawable.ic_attach),
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.size(18.dp),
                        )
                        Spacer(Modifier.width(6.dp))
                        Column(Modifier.widthIn(max = 200.dp)) {
                            Text(a.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                Formatter.formatShortFileSize(context, a.data.size.toLong()),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * The attachment written to the app's cache (`attachments/<uid>/`, shared
 * through the FileProvider), as iOS does in its temporary folder.
 */
internal fun attachmentFile(context: Context, uid: UInt, a: Attachment): File? = runCatching {
    val dir = File(context.cacheDir, "attachments/$uid").apply { mkdirs() }
    val safe = a.name.replace('/', '_').replace('\\', '_').replace(':', '_').trim().trimStart('.')
    File(dir, safe.ifEmpty { "attachment" }).apply { writeBytes(a.data) }
}.getOrNull()

/** The MIME type of a file by its extension (unknown: generic data). */
internal fun mimeOf(name: String): String {
    val ext = name.substringAfterLast('.', "").lowercase()
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
}

/** Opens the file in another app; false when none can show it. */
private fun openAttachment(context: Context, file: File): Boolean {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val view = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, mimeOf(file.name))
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    view.clipData = ClipData.newRawUri(file.name, uri)
    return try {
        // The system asks which app when there are several.
        context.startActivity(view)
        true
    } catch (_: ActivityNotFoundException) {
        false
    }
}
