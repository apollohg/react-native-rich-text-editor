package com.apollohg.editor

import android.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorStyleSheetTest {
    @Test
    fun `editor marker baseline ignores noncontiguous quote rules`() {
        fun markerAppearance(rules: String? = null): Pair<Int, Int> {
            val ruleJSON = rules?.let { ",\"rules\":$it" } ?: ""
            val theme = EditorTheme.fromJson(
                """{"version":1,"styles":{"text":{"fontSize":20},"blockquote":{"fontSize":30,"color":"#00ff00ff"},"listItem":{"fontSize":40,"color":"#0000ffff"},"listMarker":{"scale":1,"gap":0}}$ruleJSON}"""
            )!!
            val rendered = RenderBridge.buildSpannable(
                """[
                {"type":"blockStart","nodeType":"blockquote","depth":0},
                {"type":"blockStart","nodeType":"listItem","depth":1,"listContext":{"ordered":false,"index":1,"isFirst":true,"isLast":true}},
                {"type":"blockStart","nodeType":"paragraph","depth":1},
                {"type":"textRun","text":"nested","marks":[]},
                {"type":"blockEnd"},{"type":"blockEnd"},{"type":"blockEnd"}
                ]""",
                17f,
                Color.BLACK,
                theme,
                1f
            )
            val width = rendered.getSpans(0, rendered.length, CenteredBulletSpan::class.java)
                .single().getSize(android.graphics.Paint(), rendered, 0, 1, null)
            val color = rendered.getSpans(0, 1, android.text.style.ForegroundColorSpan::class.java)
                .last().foregroundColor
            return width to color
        }
        val noncontiguous =
            """{"path":["blockquote","paragraph"],"style":{"fontSize":70,"color":"#ff0000ff"}}"""
        val contiguous =
            """{"path":["blockquote","bulletList","listItem","paragraph"],"style":""" +
                """{"fontSize":50,"color":"#ff0000ff"}}"""
        assertEquals(10 to Color.GREEN, markerAppearance())
        assertEquals(10 to Color.GREEN, markerAppearance("[]"))
        assertEquals(10 to Color.GREEN, markerAppearance("[$noncontiguous]"))
        assertEquals(16 to Color.RED, markerAppearance("[$noncontiguous,$contiguous]"))
        assertEquals(
            16 to Color.BLUE,
            markerAppearance(
                """[$contiguous,{"path":["listItem","listMarker"],"style":{"color":"#0000ffff"}}]"""
            )
        )
    }

    @Test
    fun `editor list markers inherit contextual paragraph typography`() {
        fun markerAppearance(rules: String): Pair<Int, Int> {
            val theme = EditorTheme.fromJson(
                """{"version":1,"styles":{"text":{"fontSize":20},"listItem":{"fontSize":40,"color":"#0000ffff"},"listMarker":{"scale":1,"gap":0}},"rules":$rules}"""
            )!!
            val rendered = RenderBridge.buildSpannable(
                """[
                {"type":"blockStart","nodeType":"listItem","depth":1,"listContext":{"ordered":false,"index":1,"isFirst":true,"isLast":true}},
                {"type":"blockStart","nodeType":"paragraph","depth":1},
                {"type":"textRun","text":"nested","marks":[]},
                {"type":"blockEnd"},{"type":"blockEnd"}
                ]""",
                17f,
                Color.BLACK,
                theme,
                1f
            )
            val width = rendered.getSpans(0, rendered.length, CenteredBulletSpan::class.java)
                .single().getSize(android.graphics.Paint(), rendered, 0, 1, null)
            val color = rendered.getSpans(0, 1, android.text.style.ForegroundColorSpan::class.java)
                .last().foregroundColor
            return width to color
        }
        assertEquals(7 to Color.BLACK, markerAppearance("[]"))
        assertEquals(
            16 to Color.RED,
            markerAppearance(
                """[{"path":["listItem","paragraph"],"style":{"fontSize":50,"color":"#ff0000ff"}}]"""
            )
        )
        assertEquals(
            7 to Color.BLACK,
            markerAppearance(
                """[{"path":["blockquote","paragraph"],"style":{"fontSize":50,"color":"#ff0000ff"}}]"""
            )
        )
    }

    @Test
    fun `editor content and placeholder rules use empty ancestry`() {
        val editor = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        editor.placeholderText = "Write"
        editor.applyTheme(
            EditorTheme.fromJson(
                """{
            "version":1,"styles":{},"rules":[
            {"path":["content"],"style":{"paddingLeft":23}},
            {"path":["placeholder"],"style":{"color":"#ff0000ff"}},
            {"path":["paragraph","placeholder"],"style":{"color":"#00ff00ff"}}
            ]
            }"""
            )
        )
        assertEquals((23 * editor.resources.displayMetrics.density).toInt(), editor.paddingLeft)
        val layout = editor.buildPlaceholderLayout(200)!!
        val paint = android.text.TextPaint()
        (layout.text as android.text.Spanned).getSpans(
            0,
            1,
            EditorResolvedTextSpan::class.java
        ).single().updateDrawState(paint)
        assertEquals(Color.RED, paint.color)
    }

    @Test
    fun `editor mention rules retain local overrides`() {
        val theme = EditorTheme.fromJson(
            """{
            "version":1,"styles":{"mention":{"paddingLeft":3}},
            "rules":[{"path":["paragraph","mention"],"style":{"paddingLeft":17}}]
            }"""
        )!!
        val rendered = RenderBridge.buildSpannable(
            """[
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"opaqueInlineAtom","nodeType":"mention","docPos":0,"label":"same"},
            {"type":"opaqueInlineAtom","nodeType":"mention","docPos":1,"label":"same","mentionTheme":{"node":{"style":{"paddingLeft":5}}}},
            {"type":"blockEnd"}
            ]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val spans = rendered.getSpans(0, rendered.length, EditorMentionSpan::class.java).sortedBy {
            rendered.getSpanStart(it)
        }
        val paint = android.graphics.Paint()
        assertEquals(
            12,
            spans[0].getSize(paint, rendered, 0, 4, null) -
                spans[1].getSize(paint, rendered, 4, 8, null)
        )
    }

    @Test
    fun `editor special elements resolve their owning ancestry`() {
        val theme = EditorTheme.fromJson(
            """{
            "version":1,"styles":{"taskCheckbox":{"size":20,"checked":{"size":26}},"image":{"paddingLeft":2}},"rules":[
            {"path":["taskItem","taskCheckbox"],"style":{"size":30,"checked":{"size":34}}},
            {"path":["paragraph","bold"],"style":{"color":"#ff0000ff"}},
            {"path":["blockquote","horizontalRule"],"style":{"marginTop":19,"height":7}},
            {"path":["blockquote","image"],"style":{"paddingLeft":11,"resizeMode":"contain"}}
            ]
            }"""
        )!!
        val rendered = RenderBridge.buildSpannable(
            """[
            {"type":"blockStart","nodeType":"blockquote","depth":0},
            {"type":"blockStart","nodeType":"taskItem","depth":1,"listContext":{"kind":"task","checked":true,"isFirst":true,"isLast":true}},
            {"type":"blockStart","nodeType":"paragraph","depth":1},
            {"type":"textRun","text":"marked","marks":["bold"]},
            {"type":"blockEnd"},{"type":"blockEnd"},
            {"type":"voidBlock","nodeType":"horizontalRule","docPos":20},
            {"type":"voidBlock","nodeType":"image","docPos":21,"attrs":{"src":"invalid","width":40,"height":20}},
            {"type":"blockEnd"}
            ]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val checkbox = rendered.getSpans(
            0,
            rendered.length,
            EditorCheckboxSpan::class.java
        ).single()
        assertEquals(34, checkbox.getSize(android.graphics.Paint(), rendered, 0, 1, null))
        val image = rendered.getSpans(0, rendered.length, BlockImageSpan::class.java).single()
        assertEquals(11f, image.imageStyle!!.box.padding.left)
        assertEquals("contain", image.imageStyle!!.resizeMode)
        val rule = rendered.getSpans(0, rendered.length, EditorBlockBoxSpan::class.java).single {
            it.nodeType ==
                "horizontalRule"
        }
        assertEquals(19f, rule.box.margin.top)
        val markStart = rendered.indexOf("marked")
        val paint = android.text.TextPaint()
        rendered.getSpans(markStart, markStart + 1, EditorResolvedTextSpan::class.java).forEach {
            it.updateDrawState(paint)
        }
        assertEquals(Color.RED, paint.color)
    }

    @Test
    fun `editor boxes use ancestry prefixes and preserve structural list containers`() {
        val theme = EditorTheme.fromJson(
            """{
            "version":1,"styles":{"paragraph":{"marginBottom":8}},"rules":[
            {"path":["listItem","paragraph"],"style":{"marginBottom":0}},
            {"path":["blockquote","bulletList"],"style":{"paddingLeft":13,"indent":41}},
            {"path":["listItem","listMarker"],"style":{"scale":1,"gap":12,"color":"#ff0000ff"}},
            {"path":["paragraph","listMarker"],"style":{"gap":99}},
            {"path":["blockquote","bulletList","listItem","paragraph"],"style":{"paddingLeft":7}}
            ]
            }"""
        )!!
        val rendered = RenderBridge.buildSpannable(
            """[
            {"type":"blockStart","nodeType":"blockquote","depth":0},
            {"type":"blockStart","nodeType":"listItem","depth":1,"listContext":{"ordered":false,"index":1,"isFirst":true,"isLast":true}},
            {"type":"blockStart","nodeType":"paragraph","depth":1},
            {"type":"textRun","text":"nested","marks":["bold"]},
            {"type":"blockEnd"},{"type":"blockEnd"},{"type":"blockEnd"},
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"textRun","text":"top","marks":[]},{"type":"blockEnd"}
            ]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val boxes = rendered.getSpans(0, rendered.length, EditorBlockBoxSpan::class.java)
        val paragraphs = boxes.filter {
            it.nodeType == "paragraph"
        }.sortedBy { rendered.getSpanStart(it) }
        assertEquals(0f, paragraphs[0].box.margin.bottom)
        assertEquals(8f, paragraphs[1].box.margin.bottom)
        assertEquals(7f, paragraphs[0].box.padding.left)
        assertEquals(13f, boxes.single { it.nodeType == "bulletList" }.box.padding.left)
        assertEquals(
            theme.styleSheet!!.box("blockquote").outerInset.left + 13f,
            paragraphs[0].ancestorInset.left
        )
        val bullet = rendered.getSpans(0, rendered.length, CenteredBulletSpan::class.java).single()
        assertEquals(18, bullet.getSize(android.graphics.Paint(), rendered, 0, 1, null))
    }

    @Test
    fun `contextual box rules preserve sparse base values and explicit zeros`() {
        val sheet = EditorTheme.fromJson(
            """{"version":1,"styles":{"paragraph":{"marginBottom":8,"paddingLeft":3}},""" +
                """"rules":[{"path":["listItem","paragraph"],"style":{"marginBottom":0}}]}"""
        )!!.styleSheet!!

        assertEquals(8f, sheet.box("paragraph").margin.bottom)
        assertEquals(0f, sheet.box("paragraph", listOf("list_item")).margin.bottom)
        assertEquals(3f, sheet.box("paragraph", listOf("list_item")).padding.left)
    }

    @Test
    fun `rules match only contiguous canonical ancestry suffixes`() {
        data class Case(val path: String, val ancestors: List<String>, val matches: Boolean)

        val cases = listOf(
            Case("\"paragraph\"", emptyList(), true),
            Case("\"paragraph\"", listOf("blockquote", "bullet_list", "list_item"), true),
            Case(
                "\"listItem\",\"paragraph\"",
                listOf("blockquote", "bullet_list", "list_item"),
                true
            ),
            Case(
                "\"bulletList\",\"listItem\",\"paragraph\"",
                listOf("blockquote", "bullet_list", "list_item"),
                true
            ),
            Case(
                "\"blockquote\",\"listItem\",\"paragraph\"",
                listOf("blockquote", "list_item"),
                true
            ),
            Case(
                "\"blockquote\",\"listItem\",\"paragraph\"",
                listOf("blockquote", "bullet_list", "list_item"),
                false
            ),
            Case(
                "\"blockquote\",\"bulletList\",\"listItem\",\"paragraph\"",
                listOf("bullet_list", "list_item"),
                false
            ),
            Case("\"listItem\",\"paragraph\"", listOf("blockquote"), false)
        )

        cases.forEachIndexed { index, case ->
            val sheet = EditorTheme.fromJson(
                """{"version":1,"styles":{"paragraph":{"marginBottom":8}},"rules":[""" +
                    """{"path":[${case.path}],"style":{"marginBottom":17}}]}"""
            )!!.styleSheet!!
            assertEquals(
                "case $index",
                if (case.matches) 17f else 8f,
                sheet.box("paragraph", case.ancestors).margin.bottom
            )
        }
    }

    @Test
    fun `matching rules apply in declaration order including later shorter paths`() {
        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {"paragraph": {"marginBottom": 8, "paddingLeft": 3}},
                "rules": [
                    {"path": ["listItem", "paragraph"], "style": {"marginBottom": 2, "paddingLeft": 9}},
                    {"path": ["paragraph"], "style": {"marginBottom": 1}}
                ]
            }
            """.trimIndent()
        )!!.styleSheet!!

        val box = sheet.box("paragraph", listOf("list_item"))
        assertEquals(1f, box.margin.bottom)
        assertEquals(9f, box.padding.left)
    }

    @Test
    fun `element rules overlay special properties without filling sparse fields`() {
        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {
                    "bulletList": {"indent": 24, "baseIndentMultiplier": 2},
                    "listMarker": {"scale": 0.8, "gap": 6, "ordered": {"schemes": ["upperAlpha"], "suffix": "."}},
                    "taskCheckbox": {"size": 20, "gap": 4, "checkColor": "#ffffffff", "checked": {"backgroundColor": "#ff0000ff", "borderRightWidth": 3}},
                    "image": {"resizeMode": "cover", "paddingLeft": 5},
                    "horizontalRule": {"height": 2}
                },
                "rules": [
                    {"path": ["blockquote", "bulletList"], "style": {"indent": 0, "baseIndentMultiplier": 0}},
                    {"path": ["listMarker"], "style": {"scale": 0, "gap": 0, "ordered": {"suffix": ")"}}},
                    {"path": ["taskCheckbox"], "style": {"size": 0, "gap": 0, "checkColor": "#00000000", "checked": {"backgroundColor": "#00ff00ff", "borderLeftWidth": 0}}},
                    {"path": ["image"], "style": {"resizeMode": "stretch"}},
                    {"path": ["horizontalRule"], "style": {"height": 0}}
                ]
            }
            """.trimIndent()
        )!!.styleSheet!!

        val list = sheet.resolveElement("bullet_list", listOf("blockquote"))!!
        assertEquals(0f, list.indent)
        assertEquals(0f, list.baseIndentMultiplier)

        val marker = sheet.resolveElement("listMarker")!!
        assertEquals(0f, marker.scale)
        assertEquals(0f, marker.gap)
        assertEquals(listOf(EditorOrderedListNumberingScheme.UPPER_ALPHA), marker.ordered!!.schemes)
        assertEquals(")", marker.ordered!!.suffix)

        val checkbox = sheet.resolveElement("taskCheckbox")!!
        assertEquals(0f, checkbox.size)
        assertEquals(0f, checkbox.gap)
        assertEquals(Color.TRANSPARENT, checkbox.checkColor)
        assertEquals(Color.GREEN, checkbox.checked!!.box.backgroundColor)
        assertEquals(0f, checkbox.checked!!.box.border.left)
        assertEquals(3f, checkbox.checked!!.box.border.right)

        val image = sheet.resolveElement("image")!!
        assertEquals("stretch", image.resizeMode)
        assertEquals(5f, image.box.padding.left)
        assertEquals(0f, sheet.resolveElement("horizontal_rule")!!.height)
    }

    @Test
    fun `text rules follow element styles and precede each mark cascade slot`() {
        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {
                    "paragraph": {"color": "#110000ff"},
                    "bold": {"color": "#220000ff"},
                    "link": {"color": "#550000ff"},
                    "blockquote": {"fontSize": 23}
                },
                "rules": [
                    {"path": ["paragraph"], "style": {"color": "#330000ff"}},
                    {"path": ["listItem", "paragraph", "bold"], "style": {"color": "#440000ff", "fontWeight": "normal"}},
                    {"path": ["blockquote"], "style": {"fontSize": 40}}
                ]
            }
            """.trimIndent()
        )!!.styleSheet!!

        assertEquals(Color.rgb(51, 0, 0), sheet.resolveText("paragraph", listOf("list_item")).color)
        val marked = sheet.resolveText("paragraph", listOf("list_item"), listOf("strong"))
        assertEquals(Color.rgb(68, 0, 0), marked.color)
        assertEquals("normal", marked.fontWeight)
        assertEquals(
            Color.rgb(85, 0, 0),
            sheet.resolveText("paragraph", listOf("list_item"), listOf("strong", "link")).color
        )
        assertEquals(23f, sheet.resolveText("paragraph", listOf("blockquote")).fontSize)
    }

    @Test
    fun `empty and unmatched rules preserve existing element identity`() {
        val empty = EditorTheme.fromJson(
            """{"version":1,"styles":{"image":{"resizeMode":"cover"}},"rules":[]}"""
        )!!.styleSheet!!
        val emptyStyle = EditorTheme.fromJson(
            """{"version":1,"styles":{"image":{"resizeMode":"cover","paddingLeft":5}},""" +
                """"rules":[{"path":["image"],"style":{}}]}"""
        )!!.styleSheet!!
        val unmatched = EditorTheme.fromJson(
            """{"version":1,"styles":{"image":{"resizeMode":"cover"}},"rules":[""" +
                """{"path":["blockquote","image"],"style":{"resizeMode":"stretch"}}]}"""
        )!!.styleSheet!!
        val ruleOnly = EditorTheme.fromJson(
            """{"version":1,"rules":[{"path":["image"],"style":{"resizeMode":"stretch"}}]}"""
        )!!.styleSheet!!

        assertSame(empty["image"], empty.resolveElement("image"))
        assertEquals("cover", emptyStyle.resolveElement("image")!!.resizeMode)
        assertEquals(5f, emptyStyle.box("image").padding.left)
        assertSame(unmatched["image"], unmatched.resolveElement("image"))
        assertNull(unmatched.resolveElement("paragraph"))
        assertEquals("stretch", ruleOnly.resolveElement("image")!!.resizeMode)
    }

    @Test
    fun `version one stylesheet preserves box defaults and explicit zeros`() {
        val defaults = EditorTheme.fromJson("""{"version":1}""")!!.styleSheet!!
        assertEquals(0f, defaults.box("paragraph").margin.bottom)
        assertEquals(4f, defaults.box("listItem").margin.bottom)

        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {
                    "paragraph": {
                        "marginTop": 0,
                        "marginRight": 7,
                        "marginBottom": 0,
                        "marginLeft": 5,
                        "paddingTop": 3,
                        "paddingRight": 4,
                        "paddingBottom": 0,
                        "paddingLeft": 2,
                        "borderTopWidth": 1,
                        "borderRightWidth": 0,
                        "borderBottomWidth": 6,
                        "borderLeftWidth": 0
                    }
                }
            }
            """.trimIndent()
        )!!.styleSheet!!
        assertEquals(EditorEdges(0f, 7f, 0f, 5f), sheet.box("paragraph").margin)
        assertEquals(EditorEdges(3f, 4f, 0f, 2f), sheet.box("paragraph").padding)
        assertEquals(EditorEdges(1f, 0f, 6f, 0f), sheet.box("paragraph").border)
    }

    @Test
    fun `version one stylesheet preserves inherited text cascade`() {
        val sheet = EditorTheme.fromJson(
            """
            {
                "version": 1,
                "styles": {
                    "text": {
                        "fontFamily": "serif",
                        "fontSize": 19,
                        "lineHeight": 28,
                        "color": "#11223380"
                    },
                    "blockquote": {"fontSize": 21},
                    "paragraph": {"fontWeight": "700", "letterSpacing": 0}
                }
            }
            """.trimIndent()
        )!!.styleSheet!!

        assertEquals(
            EditorTextStyle(
                fontFamily = "serif",
                fontSize = 21f,
                fontWeight = "700",
                color = Color.argb(128, 17, 34, 51),
                lineHeight = 28f,
                letterSpacing = 0f
            ),
            sheet.resolveText("paragraph", ancestors = listOf("blockquote"))
        )
    }

    @Test
    fun `placeholder measures explicit line height and typography`() {
        val editor = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        editor.placeholderText = "Placeholder"
        editor.applyTheme(
            EditorTheme.fromJson(

                """{"version":1,"styles":{"placeholder":{"fontSize":18""" +
                    ""","lineHeight":40,"fontWeight":"70""" +
                    """0","textDecorationLine":"underline"}}}"""
            )
        )
        val layout = requireNotNull(editor.buildPlaceholderLayout(200))
        assertEquals(40, layout.height)
        assertTrue(layout.paint.isUnderlineText)
    }

    @Test
    fun `horizontal rule keeps styled margins and borders in line metrics`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"horizontalRule":{"height":2""" +
                ""","marginTop":3,"marginBottom":5,"borderTopWidth":4}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(
            """[{"type":"voidBlock","nodeType":"horizontalRule"}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val box = rendered.getSpans(
            0,
            rendered.length,
            EditorBlockBoxSpan::class.java
        ).singleOrNull()
        assertNotNull(box)
        assertEquals(7f, box!!.box.outerInset.top)
        assertEquals(5f, box.box.outerInset.bottom)
    }

    @Test
    fun `theme only changes preserve composing text and selection`() {
        val editor = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        editor.applyRenderJSON(

            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"compose","marks":[]},""" +
                """{"type":"blockEnd"}]"""
        )
        editor.setSelection(3)
        android.view.inputmethod.BaseInputConnection.setComposingSpans(editor.editableText)
        editor.applyTheme(
            EditorTheme.fromJson("""{"version":1,"styles":{"paragraph":{"color":"#ff0000ff"}}}""")
        )
        assertEquals("compose", editor.text.toString())
        assertEquals(3, editor.selectionStart)
        assertEquals(
            0,
            android.view.inputmethod.BaseInputConnection.getComposingSpanStart(editor.editableText)
        )
    }

    @Test
    fun `container box starts after preceding paragraph separator`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"blockquote":{"backgroundColor":"#ff0000f""" +
                """f","paddingTop":8}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(

            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"before","marks":[]},""" +
                """{"type":"blockEnd"},{"type":"blockStart","nodeType":"blockquot""" +
                """e","depth":0},{"type":"blockStart","nodeType":"paragrap""" +
                """h","depth":1},{"type":"textRun","text":"quote","marks":[]},""" +
                """{"type":"blockEnd"},{"type":"blockEnd"}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val box = rendered.getSpans(0, rendered.length, EditorBlockBoxSpan::class.java).single {
            it.box.backgroundColor ==
                Color.RED
        }
        assertEquals(rendered.indexOf("quote"), rendered.getSpanStart(box))
    }

    @Test
    fun `theme updates reuse image ownership and clear existing boxes`() {
        val editor = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        editor.applyRenderJSON(

            """[{"type":"voidBlock","nodeType":"imag""" +
                """e","attrs":{"src":"invalid-source","width":40,"height":20}}]"""
        )
        val initial = editor.text!!.getSpans(0, 1, BlockImageSpan::class.java).single()
        editor.applyTheme(
            EditorTheme.fromJson("""{"version":1,"styles":{"image":{"borderTopLeftRadius":8}}}""")
        )
        assertSame(initial, editor.text!!.getSpans(0, 1, BlockImageSpan::class.java).single())
        assertEquals(8f, initial.imageStyle?.box?.corners?.topLeft)
        editor.applyTheme(null)
        assertNull(initial.imageStyle)
        initial.close()
    }

    @Test
    fun `task checkbox size reserves its actual marker width`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"taskCheckbox":{"size":28,"gap":9""" +
                ""","checked":{"backgroundColor":"#ff0000ff"}}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(

            """[{"type":"blockStart","nodeType":"taskItem","depth":0""" +
                ""","listContext":{"kind":"task","checked":true,"isFirst":true""" +
                ""","isLast":true}},{"type":"blockStart","nodeType":"paragrap""" +
                """h","depth":1},{"type":"textRun","text":"task","marks":[]},""" +
                """{"type":"blockEnd"},{"type":"blockEnd"}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val marker = rendered.getSpans(
            0,
            1,
            android.text.style.ReplacementSpan::class.java
        ).singleOrNull()
        assertNotNull(marker)
        assertEquals(28, marker!!.getSize(android.graphics.Paint(), rendered, 0, 1, null))
    }

    @Test
    fun `mention rich override keeps inherited sides while replacing explicit typography`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"mention":{"fontSize":22""" +
                ""","borderLeftWidth":4,"borderRightWidth":3,"color":"#ff0000ff"}}""" +
                ""","mentions":{"node":{"style":{"fontSize":19""" +
                ""","borderRightWidth":0}}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(

            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"opaqueInlineAtom","nodeType":"mention","label":"@Ad""" +
                """a","docPos":1},{"type":"blockEnd"}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val span = rendered.getSpans(
            0,
            4,
            android.text.style.ReplacementSpan::class.java
        ).singleOrNull()
        assertNotNull(span)
        val paint = android.text.TextPaint().apply { textSize = 19f }
        assertTrue(span!!.getSize(paint, rendered, 0, 4, null) >= paint.measureText("@Ada") + 4)
        val metrics = android.graphics.Paint.FontMetricsInt()
        EditorMentionSpan(
            EditorElementStyle(EditorTextStyle(fontSize = 19f, lineHeight = 40f), EditorBoxStyle()),
            1f
        ).getSize(paint, "Ada", 0, 3, metrics)
        assertEquals(40, metrics.descent - metrics.ascent)
    }

    @Test
    fun `image border padding and margins reserve replacement geometry`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"image":{"paddingLeft":3,"paddingTop":2""" +
                ""","borderRightWidth":4,"marginBottom":5}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(

            """[{"type":"voidBlock","nodeType":"imag""" +
                """e","attrs":{"src":"invalid-source","width":40,"height":20}}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val span = rendered.getSpans(0, 1, BlockImageSpan::class.java).single()
        val metrics = android.graphics.Paint.FontMetricsInt()
        assertEquals(47, span.getSize(android.graphics.Paint(), rendered, 0, 1, metrics))
        assertEquals(27, metrics.descent - metrics.ascent)
        span.close()
    }

    @Test
    fun `content border participates in host padding and clearing`() {
        val editor = EditorEditText(org.robolectric.RuntimeEnvironment.getApplication())
        editor.applyTheme(
            EditorTheme.fromJson(

                """{"version":1,"styles":{"content":{"borderLeftWidth":3""" +
                    ""","paddingLeft":7,"paddingTop":5,"borderTopWidth":2}}}"""
            )
        )
        val density = editor.resources.displayMetrics.density
        assertEquals((10 * density).toInt(), editor.paddingLeft)
        assertEquals((7 * density).toInt(), editor.paddingTop)
        editor.applyTheme(null)
        assertEquals(0, editor.paddingLeft)
    }

    @Test
    fun `inline explicit normal and none clear semantic marks in fixed order`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"bold":{"color":"#ff0000ff"}""" +
                ""","link":{"fontWeight":"normal","textDecorationLine":"non""" +
                """e","letterSpacing":2}}}"""
        )!!
        val rendered = RenderBridge.buildSpannable(

            """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                """{"type":"textRun","text":"link","marks":["bold",{"type":"lin""" +
                """k","href":"https://example.com"}]},{"type":"blockEnd"}]""",
            17f,
            Color.BLACK,
            theme,
            1f
        )
        val paint = android.text.TextPaint()
        rendered.getSpans(0, 4, android.text.style.CharacterStyle::class.java).forEach {
            it.updateDrawState(paint)
        }
        assertFalse(paint.typeface?.isBold == true)
        assertFalse(paint.isUnderlineText)
        assertEquals(2f / 17f, paint.letterSpacing, 0.001f)
    }

    @Test
    fun `paragraph vertical box space is included in layout`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"paragraph":{"paddingTop":8""" +
                ""","paddingBottom":10,"borderTopWidth":2,"marginTop":3""" +
                ""","marginBottom":5}}}"""
        )!!
        fun height(value: EditorTheme?): Int {
            val rendered = RenderBridge.buildSpannable(

                """[{"type":"blockStart","nodeType":"paragraph","depth":0},""" +
                    """{"type":"textRun","text":"box","marks":[]},{"type":"blockEnd"}]""",
                17f,
                Color.BLACK,
                value,
                1f
            )
            return android.text.StaticLayout.Builder.obtain(
                rendered,
                0,
                rendered.length,
                android.text.TextPaint().apply {
                    textSize =
                        17f
                },
                200
            ).setIncludePad(false).build().height
        }
        assertEquals(
            28,
            height(theme) - height(EditorTheme.fromJson("""{"version":1,"styles":{}}"""))
        )
    }

    @Test
    fun `versioned paragraph inherits base typography and portable alpha`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"text":{"fontSize":19,"lineHeight":28""" +
                ""","color":"#11223380"},"paragraph":{"marginBottom":12}}}"""
        )!!
        val paragraph = theme.effectiveTextStyle("paragraph")
        assertEquals(19f, paragraph.fontSize)
        assertEquals(28f, paragraph.lineHeight)
        assertEquals(Color.argb(128, 17, 34, 51), paragraph.color)
    }

    @Test
    fun `heading semantic size precedes ancestor overrides`() {
        val theme = EditorTheme.fromJson(

            """{"version":1,"styles":{"text":{"fontSize":19}""" +
                ""","blockquote":{"fontSize":23},"h1":{"color":"#ff0000ff"}}}"""
        )!!
        assertEquals(32f, theme.effectiveTextStyle("h1").fontSize)
        assertEquals(23f, theme.effectiveTextStyle("h1", true).fontSize)
    }

    @Test
    fun `unknown stylesheet versions are rejected`() {
        assertNull(EditorTheme.fromJson("""{"version":2,"styles":{}}"""))
    }
}
