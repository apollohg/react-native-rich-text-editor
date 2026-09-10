package com.apollohg.editor

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorStyleSheetThemeAdmissionTest : NativeEditorExpoViewTestSupport() {
    @Test
    fun `malformed native rules preserve base styles and valid sibling rules`() {
        val malformedField = EditorTheme.fromJson(
            """{"version":1,"styles":{"paragraph":{"marginBottom":8}},"rules":{}}"""
        )!!.styleSheet!!
        assertEquals(8f, malformedField.box("paragraph").margin.bottom)

        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {"paragraph": {"marginBottom": 8, "paddingLeft": 3}},
                "rules": [
                    null,
                    {"path": [], "style": {"marginBottom": 7}},
                    {"path": ["unknown"], "style": {"marginBottom": 6}},
                    {"path": [1, "paragraph"], "style": {"marginBottom": 5}},
                    {"path": ["paragraph"], "style": []},
                    {"path": ["paragraph"]},
                    {"path": ["paragraph"], "style": {"marginBottom": 2}}
                ]
            }
            """.trimIndent()
        )!!.styleSheet!!

        assertEquals(2f, sheet.box("paragraph").margin.bottom)
        assertEquals(3f, sheet.box("paragraph").padding.left)
    }

    @Test
    fun `invalid native theme preserves previous presentation`() {
        val context = testExpoContext(RuntimeEnvironment.getApplication())
        val view = NativeEditorExpoView(context.context, context.appContext)
        val valid = """{"version":1,"styles":{"paragraph":{"fontSize":21}}}"""
        view.applyThemeJson(valid)
        val previous = view.richTextView.editorEditText.theme
        for (invalid in listOf(
            """{"version":2,"styles":{}}""",
            """{"version":"1","styles":{}}""",
            """{"version":1,"styles":[]}"""
        )) {
            assertThrows(IllegalArgumentException::class.java) { view.applyThemeJson(invalid) }
            assertEquals(valid, view.lastThemeJson)
            assertSame(previous, view.richTextView.editorEditText.theme)
        }
    }
}
