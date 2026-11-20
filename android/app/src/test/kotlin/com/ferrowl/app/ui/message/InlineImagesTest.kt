package com.ferrowl.app.ui.message

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InlineImagesTest {
    private val gif = byteArrayOf(0x47, 0x49, 0x46, 0x38, 0x39, 0x61)
    private val png = byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x0D, 0x0A)

    @Test
    fun cidBecomesDataUrl() {
        val out = withInlineImages("<img src=\"CID:logo@shop\"><img src=\"cid:icon@shop\">", mapOf("logo@shop" to gif, "icon@shop" to png))
        assertTrue(out.contains("src=\"data:image/gif;base64,R0lGODlh\""))
        assertTrue(out.contains("src=\"data:image/png;base64,"))
        assertFalse(out.contains("cid:", ignoreCase = true))
    }

    @Test
    fun picturesByTheirBytes() {
        assertEquals("image/gif", imageMime(gif))
        assertEquals("image/png", imageMime(png))
        assertEquals("image/jpeg", imageMime(byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte(), 0)))
        assertEquals("application/octet-stream", imageMime(byteArrayOf(1, 2, 3)))
    }
}

class PreviewKindTest {
    @Test
    fun byExtension() {
        assertEquals(PreviewKind.PDF, previewKind("F24_11.PDF"))
        assertEquals(PreviewKind.IMAGE, previewKind("attachment-1.png"))
        assertEquals(PreviewKind.TEXT, previewKind("notes.csv"))
        assertEquals(PreviewKind.NONE, previewKind("archive.zip"))
        assertEquals(PreviewKind.NONE, previewKind("attachment-3"))
    }
}
