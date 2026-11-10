package com.ferrowl.app.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

// Colors of the desktop themes (design/themes.json): Ruggine (light) and
// Notte (dark), mapped onto Material 3 roles.

private val Ruggine = lightColorScheme(
    primary = Color(0xFFA63A0C),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFFBEDE4),
    onPrimaryContainer = Color(0xFF7C2A07),
    secondaryContainer = Color(0xFFEADFD5),
    onSecondaryContainer = Color(0xFF1F1B18),
    background = Color(0xFFFFFFFF),
    onBackground = Color(0xFF1F1B18),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF1F1B18),
    surfaceVariant = Color(0xFFF3EEE9),
    onSurfaceVariant = Color(0xFF5B5650),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow = Color(0xFFFDFBF9),
    surfaceContainer = Color(0xFFFBF9F7),
    surfaceContainerHigh = Color(0xFFF6F2EE),
    surfaceContainerHighest = Color(0xFFF1ECE7),
    // Material's defaults are lilac: every role is set to the warm palette.
    tertiary = Color(0xFF7A5A3A),
    onTertiary = Color(0xFFFFFFFF),
    tertiaryContainer = Color(0xFFF1E4D6),
    onTertiaryContainer = Color(0xFF3D2A17),
    inversePrimary = Color(0xFFF3B08E),
    outline = Color(0xFFD6CDC3),
    outlineVariant = Color(0xFFE7E1DA),
    error = Color(0xFFB3261E),
)

private val Notte = darkColorScheme(
    primary = Color(0xFFE07A4A),
    onPrimary = Color(0xFF1A0D06),
    primaryContainer = Color(0xFF33231B),
    onPrimaryContainer = Color(0xFFF3B08E),
    secondaryContainer = Color(0xFF2C2F35),
    onSecondaryContainer = Color(0xFFECECEE),
    background = Color(0xFF17181B),
    onBackground = Color(0xFFECECEE),
    surface = Color(0xFF17181B),
    onSurface = Color(0xFFECECEE),
    surfaceVariant = Color(0xFF26292E),
    onSurfaceVariant = Color(0xFFA0A4AC),
    surfaceContainerLowest = Color(0xFF121316),
    surfaceContainerLow = Color(0xFF1A1C1F),
    surfaceContainer = Color(0xFF1D1F23),
    surfaceContainerHigh = Color(0xFF1B1D21),
    surfaceContainerHighest = Color(0xFF2A2D32),
    tertiary = Color(0xFFD9B48C),
    onTertiary = Color(0xFF2E1F10),
    tertiaryContainer = Color(0xFF3A2C1F),
    onTertiaryContainer = Color(0xFFF1E4D6),
    inversePrimary = Color(0xFFA63A0C),
    outline = Color(0xFF41454D),
    outlineVariant = Color(0xFF2C2F35),
    error = Color(0xFFF28B82),
)

/** Pictures in HTML mail load from the network (Settings › Load remote images). */
val LocalRemoteImages = staticCompositionLocalOf { true }

/**
 * App theme: one of design/themes.json chosen in the settings, or "auto"
 * (Ruggine, Notte when the system is dark).
 */
@Composable
fun FerrowlTheme(theme: String = "auto", content: @Composable () -> Unit) {
    val scheme = THEMES[theme] ?: if (isSystemInDarkTheme()) Notte else Ruggine
    MaterialTheme(colorScheme = scheme, content = content)
}
