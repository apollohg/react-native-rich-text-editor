package com.apollohg.editor

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class EditorMentionQueryStateTest {
    @Test
    fun `caret directly before a trigger has no mention query`() {
        assertNull(
            resolveMentionQueryState(
                text = TEXT,
                cursorScalar = TEXT.indexOf(TRIGGER),
                trigger = TRIGGER,
                isCaretInsideMention = false
            )
        )
    }

    @Test
    fun `caret after a trigger reports the typed query`() {
        assertEquals(
            MentionQueryState(
                query = QUERY,
                trigger = TRIGGER,
                anchor = TEXT.indexOf(TRIGGER),
                head = TEXT.length
            ),
            resolveMentionQueryState(
                text = TEXT,
                cursorScalar = TEXT.length,
                trigger = TRIGGER,
                isCaretInsideMention = false
            )
        )
    }

    private companion object {
        const val TRIGGER = "@"
        const val QUERY = "al"
        const val TEXT = "bold $TRIGGER$QUERY"
    }
}
