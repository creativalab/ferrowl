package com.ferrowl.app.ui.message

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.ferrowl.app.R
import com.ferrowl.app.ui.notice.LocalNotices
import com.ferrowl.core.MailInsight
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * What a message is about, picked out of its text by the engine
 * (`mailInsights`): a shipment to follow, a code to copy, a bill whose due
 * date can go in the calendar. Shown above the message, like an invitation.
 */
@Composable
fun InsightCards(insights: List<MailInsight>, onAddDue: (due: String, number: String, amount: String) -> Unit) {
    val context = LocalContext.current
    val notices = LocalNotices.current
    val uri = LocalUriHandler.current
    val copied = stringResource(R.string.insight_copied)
    fun copy(text: String) {
        context.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Ferrowl", text))
        notices.info(copied)
    }
    insights.forEach { insight ->
        Card(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)) {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                when (insight) {
                    is MailInsight.Shipment -> {
                        val title = stringResource(R.string.insight_shipment)
                        Text(
                            if (insight.carrier.isEmpty()) title else "$title · ${insight.carrier}",
                            color = MaterialTheme.colorScheme.primary,
                            fontWeight = FontWeight.SemiBold,
                        )
                        SelectionContainer { Text(insight.number, fontFamily = FontFamily.Monospace) }
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (insight.url.isNotEmpty()) {
                                Button(onClick = { runCatching { uri.openUri(insight.url) } }) {
                                    Text(stringResource(R.string.insight_track))
                                }
                            }
                            OutlinedButton(onClick = { copy(insight.number) }) { Text(stringResource(R.string.insight_copy)) }
                        }
                    }
                    is MailInsight.Code -> {
                        Text(stringResource(R.string.insight_code), color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
                        SelectionContainer {
                            Text(insight.code, style = MaterialTheme.typography.headlineSmall, fontFamily = FontFamily.Monospace)
                        }
                        Button(onClick = { copy(insight.code) }) { Text(stringResource(R.string.insight_copy)) }
                    }
                    is MailInsight.Payment -> {
                        val title = stringResource(R.string.insight_bill)
                        Text(
                            if (insight.number.isEmpty()) title else "$title · ${insight.number}",
                            color = MaterialTheme.colorScheme.primary,
                            fontWeight = FontWeight.SemiBold,
                        )
                        if (insight.amount.isNotEmpty()) Text(insight.amount, style = MaterialTheme.typography.titleMedium)
                        val due = runCatching { LocalDate.parse(insight.due) }.getOrNull()
                        if (due != null) {
                            Text(
                                stringResource(R.string.insight_due)
                                    .replace("{date}", due.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.LONG))),
                            )
                            OutlinedButton(onClick = { onAddDue(insight.due, insight.number, insight.amount) }) {
                                Text(stringResource(R.string.insight_add_due))
                            }
                        }
                    }
                }
            }
        }
    }
}
