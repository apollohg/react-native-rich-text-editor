package com.apollohg.editor.viewer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class PreparedStyleSheetTest {
    @Test
    fun `unchanged rule heights retain the prepared viewer minimum`() {
        fun measure(height: Int, rules: String): Int {
            val theme = PreparedProseTheme.resolve(
                """{"version":1,"styles":{"horizontalRule":{"height":$height}},"rules":$rules}""",
                1f
            )
            val block = ViewerBlock("horizontalRule", 0, false, null, null, emptyList())
            return StaticLayoutAndroidProseLayoutEngine().prepare(
                ViewerDocument("rule-height", listOf(block), false, 0),
                ProseLayoutKey("rule-height", 200, "rule-height", 0, 0, 0, 0, "rule-height"),
                theme,
                200,
                1f,
                false
            ).blocks.single().fragments.single {
                it.kind == PreparedProseFragmentKind.RULE
            }.bounds.height()
        }
        assertEquals(1, measure(0, "[]"))
        assertEquals(
            1,
            measure(0, """[{"path":["horizontalRule"],"style":{"backgroundColor":"#ff0000ff"}}]""")
        )
        assertEquals(0, measure(2, """[{"path":["horizontalRule"],"style":{"height":0}}]"""))
    }

    @Test
    fun `mixed viewer lists retain base arithmetic and resolve each container and marker prefix`() {
        val outer = ViewerListContext(false, 1, null, false, true)
        val inner = ViewerListContext(true, 2, null, false, true)
        val block =
            ViewerBlock(
                "paragraph",
                2,
                true,
                inner,
                ViewerListItemBoundary(2, 1, true, true),
                listOf(ViewerInline.Text("nested", emptyList())),
                listItemAncestors = listOf(
                    ViewerListItemAncestor(1, outer, 0, true, true),
                    ViewerListItemAncestor(2, inner, 1, true, true)
                ),
                containers = listOf(
                    "blockquote",
                    "bulletList",
                    "listItem",
                    "orderedList",
                    "listItem"
                ).mapIndexed {
                        index,
                        name
                    ->
                    ViewerContainerAncestor(index, name, 0, 0)
                }
            )
        fun measure(rules: String = "[]"): PreparedProseBlock {
            val theme = PreparedProseTheme.resolve(
                """{"version":1,"styles":{"bulletList":{"indent":20,"baseIndentMultiplier":2},"orderedList":{"indent":40,"baseIndentMultiplier":3},"listMarker":{"gap":6}},"rules":$rules}""",
                1f
            )
            return StaticLayoutAndroidProseLayoutEngine().prepare(
                ViewerDocument("mixed", listOf(block), false, 0),
                ProseLayoutKey("mixed", 600, "mixed", 0, 0, 0, 0, "mixed"),
                theme,
                600,
                1f,
                false
            ).blocks.single()
        }
        fun text(block: PreparedProseBlock) = block.fragments.single {
            it.kind ==
                PreparedProseFragmentKind.TEXT
        }
        fun markers(block: PreparedProseBlock) = block.fragments.filter {
            it.kind ==
                PreparedProseFragmentKind.MARKER
        }
        val baseline = measure()
        assertEquals(
            100,
            text(baseline).bounds.left - 13 - markers(baseline).sumOf { it.bounds.width() + 6 }
        )
        val indented = measure("""[{"path":["blockquote","bulletList"],"style":{"indent":33}}]""")
        assertEquals(39, text(indented).bounds.left - text(baseline).bounds.left)
        val styled =
            measure(
                """[{"path":["listItem","listMarker"],"style":{"gap":12,"color":"#00ff00ff","ordered":{"schemes":["upperAlpha"],"suffix":")"}}},{"path":["paragraph","listMarker"],"style":{"gap":99}}]"""
            )
        assertEquals("B)", markers(styled).last().label)
        assertTrue(markers(styled).all { it.color == android.graphics.Color.GREEN })
        assertEquals(
            12,
            text(styled).bounds.left - text(baseline).bounds.left -
                markers(styled).sumOf { it.bounds.width() } +
                markers(baseline).sumOf { it.bounds.width() }
        )
    }

    @Test
    fun `nested viewer rules update geometry paint and restore the cached baseline`() {
        val containers = listOf(
            ViewerContainerAncestor(1, "blockquote", 0, 1),
            ViewerContainerAncestor(2, "bulletList", 0, 1),
            ViewerContainerAncestor(3, "listItem", 0, 1)
        )
        val document = ViewerDocument(
            "a".repeat(64),
            listOf(0, 1, 2).map { index ->
                ViewerBlock(
                    "paragraph",
                    0,
                    index < 2,
                    null,
                    null,
                    listOf(
                        ViewerInline.Text(
                            "nested",
                            listOf(uniffi.editor_core.FfiViewerMark("bold", "{}"))
                        )
                    ),
                    containers = if (index < 2) containers else emptyList()
                )
            },
            false,
            0
        )
        val styles = """{
            "text":{"lineHeight":20},"paragraph":{"marginTop":3,"marginBottom":8},
            "blockquote":{"paddingTop":2,"paddingBottom":4}
        }"""
        val rules = """[
            {"path":["content"],"style":{"paddingLeft":5,"paddingTop":3}},
            {"path":["listItem","paragraph"],"style":{"marginTop":4,"marginBottom":0,"paddingTop":2,"paddingBottom":3,"fontSize":22,"lineHeight":40}},
            {"path":["blockquote","bulletList"],"style":{"paddingLeft":13,"paddingTop":5,"paddingBottom":6,"marginTop":11,"marginBottom":13}},
            {"path":["blockquote","bulletList","listItem","paragraph"],"style":{"paddingLeft":7}},
            {"path":["listItem","paragraph","bold"],"style":{"color":"#ff0000ff"}}
        ]"""
        val registry =
            PreparedProseLayoutRegistry(
                CountingDocumentCompiler {
                    document
                },
                StaticLayoutAndroidProseLayoutEngine()
            )
        fun measure(rules: String?): PreparedProseLayout {
            val ruleJson = rules?.let { ",\"rules\":$it" }.orEmpty()
            val theme = """{"version":1,"styles":$styles$ruleJson}"""
            return registry.measure(
                ProseViewerRequest(
                    com.apollohg.editor.ProseViewerSource.Json("{}"),
                    com.apollohg.editor.ProseViewerConfiguration(
                        configJson = "{}",
                        themeJson = theme
                    )
                ),
                300,
                1f
            )
        }
        fun text(layout: PreparedProseLayout, index: Int) = layout.blocks[index].fragments.single {
            it.kind ==
                PreparedProseFragmentKind.TEXT
        }
        val baseline = measure(null)
        val styled = measure(rules)
        val restored = measure(null)
        assertTrue(baseline === restored)
        assertEquals(2, registry.layoutPreparationCount)
        assertEquals(5f, styled.contentBox!!.padding.left)
        assertTrue(baseline.key.themeDigest != styled.key.themeDigest)
        assertEquals(baseline.heightPx, restored.heightPx)
        assertEquals(25, text(styled, 0).bounds.left - text(baseline, 0).bounds.left)
        assertEquals(22, text(styled, 0).bounds.top - text(baseline, 0).bounds.top)
        assertEquals(40, text(styled, 0).bounds.height())
        assertEquals(9, text(styled, 1).bounds.top - text(styled, 0).bounds.bottom)
        val paragraph = styled.blocks[0].fragments.last {
            it.kind ==
                PreparedProseFragmentKind.BACKGROUND
        }
        assertEquals(0f, paragraph.box!!.margin.bottom)
        assertEquals(7f, paragraph.box!!.padding.left)
        val list = styled.blocks[0].fragments.first { it.box?.padding?.left == 13f }
        assertEquals(11f, list.box!!.margin.top)
        assertEquals(13f, list.box!!.margin.bottom)
        assertEquals(text(styled, 1).bounds.bottom + 3 + 4 + 6, list.decorationBounds!!.bottom)
        assertEquals(
            8f,
            styled.blocks[2].fragments.first {
                it.kind ==
                    PreparedProseFragmentKind.BACKGROUND
            }.box!!.margin.bottom
        )
        val span = text(styled, 0).layout!!.text as android.text.Spanned
        assertEquals(
            android.graphics.Color.RED,
            span.getSpans(
                0,
                span.length,
                com.apollohg.editor.EditorResolvedTextSpan::class.java
            ).single().style.color
        )
    }

    @Test
    fun `viewer special elements and inline marks use owning ancestry`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"taskCheckbox":{"size":20,"checked":{"size":26}},"mention":{"paddingLeft":3}},"rules":[
            {"path":["taskItem","taskCheckbox"],"style":{"size":30,"gap":9,"checked":{"size":34}}},
            {"path":["paragraph","taskCheckbox"],"style":{"checked":{"size":99}}},
            {"path":["paragraph","bold"],"style":{"color":"#ff0000ff"}},
            {"path":["paragraph","mention"],"style":{"paddingLeft":17,"color":"#00ff00ff"}},
            {"path":["blockquote","horizontalRule"],"style":{"marginTop":19,"height":7,"backgroundColor":"#ff0000ff"}},
            {"path":["blockquote","image"],"style":{"paddingLeft":11,"resizeMode":"cover"}}
        ]}""",
            1f
        )
        val quote = ViewerContainerAncestor(1, "blockquote", 0, 2)
        val task = ViewerListContext(false, 1, "task", true, true)
        val document = ViewerDocument(
            "special",
            listOf(
                ViewerBlock(
                    "paragraph",
                    1,
                    true,
                    task,
                    ViewerListItemBoundary(3, 0, true, true),
                    listOf(
                        ViewerInline.Text(
                            "marked",
                            listOf(uniffi.editor_core.FfiViewerMark("bold", "{}"))
                        ),
                        ViewerInline.Atom("mention", 1, "{}", "Ada")
                    ),
                    containers = listOf(
                        quote,
                        ViewerContainerAncestor(2, "taskList", 0, 0),
                        ViewerContainerAncestor(3, "taskItem", 0, 0)
                    )
                ),
                ViewerBlock(
                    "horizontalRule",
                    1,
                    true,
                    null,
                    null,
                    emptyList(),
                    containers = listOf(quote)
                ),
                ViewerBlock(
                    "image",
                    1,
                    true,
                    null,
                    null,
                    listOf(
                        ViewerInline.Atom(
                            "image",
                            3,
                            """{"src":"test.png","width":40,"height":20}""",
                            ""
                        )
                    ),
                    containers = listOf(quote)
                )
            ),
            false,
            0
        )
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            ProseLayoutKey("special", 300, "special", 0, 0, 0, 0, "special"),
            theme,
            300,
            1f,
            false
        )
        val marker = result.blocks[0].fragments.single {
            it.kind == PreparedProseFragmentKind.MARKER
        }
        assertEquals(34, marker.bounds.width())
        val atom = result.blocks[0].fragments.single { it.kind == PreparedProseFragmentKind.ATOM }
        assertEquals(17f, atom.box!!.padding.left)
        assertEquals(android.graphics.Color.GREEN, atom.labelLayout!!.paint.color)
        val rule = result.blocks[1].fragments.single { it.kind == PreparedProseFragmentKind.RULE }
        assertEquals(7, rule.bounds.height())
        assertEquals(android.graphics.Color.RED, rule.color)
        val image = result.blocks[2].fragments.single { it.kind == PreparedProseFragmentKind.IMAGE }
        assertEquals("cover", image.resizeMode)
        assertEquals(
            11f,
            result.blocks[2].fragments.first {
                it.decorationBounds == null &&
                    it.box != null
            }.box!!.padding.left
        )
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `zero padding short mentions use label width while other atoms retain minimum width`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"text":{"fontSize":40},"mention":{"paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0}}}""",
            2f
        )
        fun prepare(nodeType: String): PreparedProseFragment {
            val document = ViewerDocument(
                "short-atom",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(ViewerInline.Atom(nodeType, 1, "{}", "i"))
                    )
                ),
                false,
                0
            )
            val key = ProseLayoutKey("short-atom", 400, "short-atom", 0, 0, 0, 0, "short-atom")
            return StaticLayoutAndroidProseLayoutEngine().prepare(
                document,
                key,
                theme,
                400,
                2f,
                false
            ).blocks.single().fragments.single { it.kind == PreparedProseFragmentKind.ATOM }
        }
        val mention = prepare("mention")
        val labelWidth = kotlin.math.ceil(mention.labelLayout!!.paint.measureText("i")).toInt()
        assertTrue(labelWidth < 80)
        assertEquals(labelWidth, mention.bounds.width())
        assertEquals(80, prepare("custom").bounds.width())
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `mention padding merges global and local sides and changes viewer geometry`() {
        fun prepare(style: String, localStyle: String = "{}"): PreparedProseFragment {
            val theme = PreparedProseTheme.resolve(
                """{"version":1,"styles":{"mention":$style}}""",
                2f
            )
            val document = ViewerDocument(
                "mention",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(
                            ViewerInline.Atom(
                                "mention",
                                1,
                                """{"mentionTheme":{"node":{"style":$localStyle}}}""",
                                "@Ada"
                            )
                        )
                    )
                ),
                false,
                0
            )
            val key = ProseLayoutKey("mention", 400, "mention", 0, 0, 0, 0, "mention")
            return StaticLayoutAndroidProseLayoutEngine().prepare(
                document,
                key,
                theme,
                400,
                2f,
                false
            ).blocks.single().fragments.single { it.kind == PreparedProseFragmentKind.ATOM }
        }
        val defaults = prepare("{}")
        assertEquals(com.apollohg.editor.EditorEdges(4f, 8f, 4f, 8f), defaults.box!!.padding)
        val zero =
            prepare("""{"paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0}""")
        assertEquals(16, defaults.bounds.width() - zero.bounds.width())
        assertEquals(8, defaults.bounds.height() - zero.bounds.height())
        val local = prepare(
            """{"paddingTop":7,"paddingRight":9}""",
            """{"paddingTop":0,"paddingLeft":6}"""
        )
        assertEquals(com.apollohg.editor.EditorEdges(0f, 18f, 4f, 12f), local.box!!.padding)
        assertEquals(12, local.labelX - local.bounds.left)
        assertEquals(0, local.labelY - local.bounds.top)
    }

    @Test
    fun `inline explicit line height reaches viewer measurement`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"text":{"lineHeight":20},"bold":{"lineHeight":40}}}""",
            1f
        )
        val document =
            ViewerDocument(
                "line",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(
                            ViewerInline.Text(
                                "bold",
                                listOf(uniffi.editor_core.FfiViewerMark("bold", "{}"))
                            )
                        )
                    )
                ),
                false,
                0
            )
        val key = ProseLayoutKey("line", 200, "line", 0, 0, 0, 0, "line")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            theme,
            200,
            1f,
            false
        )
        assertEquals(40, result.heightPx)
    }

    @Test
    fun `physical alignment remains left for RTL paragraphs`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"paragraph":{"textAlign":"left"}}}""",
            1f
        )
        val document =
            ViewerDocument(
                "rtl",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(ViewerInline.Text("שלום", emptyList()))
                    )
                ),
                false,
                0
            )
        val key = ProseLayoutKey("rtl", 200, "rtl", 0, 0, 0, 0, "rtl")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            theme,
            200,
            1f,
            false
        )
        val layout = result.blocks.single().fragments.single {
            it.kind ==
                PreparedProseFragmentKind.TEXT
        }.layout!!
        assertEquals(0f, layout.getLineLeft(0), .01f)
    }

    @Test
    fun `stylesheet retains document foreground background and font marks`() {
        val marks = listOf(
            uniffi.editor_core.FfiViewerMark("textColor", """{"color":"#ff0000"}"""),
            uniffi.editor_core.FfiViewerMark("highlight", """{"color":"#ffff00"}"""),
            uniffi.editor_core.FfiViewerMark(
                "textStyle",
                """{"fontFamily":"monospace","fontSize":29}"""
            )
        )
        val document =
            ViewerDocument(
                "marks",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(ViewerInline.Text("styled", marks))
                    )
                ),
                false,
                0
            )
        val key = ProseLayoutKey("marks", 200, "marks", 0, 0, 0, 0, "marks")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            PreparedProseTheme.resolve(
                """{"version":1,"styles":{"paragraph":{"marginBottom":4}}}""",
                1f
            ),
            200,
            1f,
            false
        )
        val text = result.blocks.single().fragments.single {
            it.kind ==
                PreparedProseFragmentKind.TEXT
        }.layout!!.text as android.text.Spanned
        val style = text.getSpans(
            0,
            text.length,
            com.apollohg.editor.EditorResolvedTextSpan::class.java
        ).single().style
        assertEquals(android.graphics.Color.RED, style.color)
        assertEquals(android.graphics.Color.YELLOW, style.backgroundColor)
        assertEquals(29f, style.fontSize)
        assertEquals("monospace", style.fontFamily)
    }

    @Test
    fun `styled image preserves declared size within available width`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"image":{"borderLeftWidth":4,"borderRightWidth":2,"paddingTop":3}}}""",
            1f
        )
        val block =
            ViewerBlock(
                "image",
                0,
                false,
                null,
                null,
                listOf(
                    ViewerInline.Atom(
                        "image",
                        1,
                        """{"src":"image.png","width":40,"height":20}""",
                        ""
                    )
                )
            )
        val key = ProseLayoutKey("test", 200, "test", 0, 0, 0, 0, "test")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            ViewerDocument("test", listOf(block), false, 0),
            key,
            theme,
            200,
            1f,
            false
        )
        assertEquals(40, result.imageAttachments.single().bounds.width())
        assertEquals(20, result.imageAttachments.single().bounds.height())
        assertEquals(23, result.heightPx)
    }

    @Test
    fun `compiler retains separate nested quote and list containers`() {
        val source =
            """{"type":"doc","content":[{"type":"blockquote",""" +
                """"content":[{"type":"paragraph","content":[{"type":"text",""" +
                """"text":"outer"}]},{"type":"blockquote",""" +
                """"content":[{"type":"bulletList","content":[{"type":"listItem",""" +
                """"content":[{"type":"paragraph","content":[{"type":"text",""" +
                """"text":"inner"}]}]}]}]}]}]}"""
        val document =
            compileWithRust(
                ProseViewerRequest(
                    com.apollohg.editor.ProseViewerSource.Json(source),
                    com.apollohg.editor.ProseViewerConfiguration(
                        configJson =
                            """{"schema":{"nodes":[{"name":"doc","content":"block+",""" +
                                """"role":"doc"},{"name":"paragraph","content":"inline*",""" +
                                """"group":"block","role":"textBlock"},{"name":"blockquote",""" +
                                """"content":"block+","group":"block","role":"block"},""" +
                                """{"name":"bulletList","content":"listItem+","group":"block",""" +
                                """"role":"list"},{"name":"listItem",""" +
                                """"content":"paragraph block*",""" +
                                """"role":"listItem"},{"name":"text","group":"inline",""" +
                                """"role":"text"}]},"initialization":{"type":"localEmpty"}}"""
                    )
                )
            )
        assertEquals(
            listOf("blockquote", "blockquote", "bulletList", "listItem"),
            document.blocks.last().containers.map {
                it.nodeType
            }
        )
        assertEquals(1, document.blocks.first().containers.single().lastLeaf)
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"blockquote":{"paddingTop":4,"paddingBottom":5,"backgroundColor":"#ff0000ff"}}}""",
            1f
        )
        val key = ProseLayoutKey("test", 200, "test", 0, 0, 0, 0, "test")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            theme,
            200,
            1f,
            false
        )
        val first = result.blocks.first().fragments.first { it.decorationBounds != null }
        val last = result.blocks.last().fragments.first { it.decorationBounds != null }
        assertEquals(first.decorationBounds, last.decorationBounds)
        assertTrue(first.bounds.bottom <= last.bounds.top)
    }

    @Test
    fun `paragraph box reserves asymmetric geometry in viewer`() {
        val theme = PreparedProseTheme.resolve(
            """{"version":1,"styles":{"paragraph":{"paddingTop":8,"paddingRight":19,"paddingBottom":10,"paddingLeft":7,"borderTopWidth":2,"borderRightWidth":3,"marginTop":3,"marginBottom":5}}}""",
            1f
        )
        val document =
            ViewerDocument(
                "test",
                listOf(
                    ViewerBlock(
                        "paragraph",
                        0,
                        false,
                        null,
                        null,
                        listOf(ViewerInline.Text("box", emptyList()))
                    )
                ),
                false,
                0
            )
        val key = ProseLayoutKey("test", 200, "test", 0, 0, 0, 0, "test")
        val result = StaticLayoutAndroidProseLayoutEngine().prepare(
            document,
            key,
            theme,
            200,
            1f,
            false
        )
        val text = result.blocks.single().fragments.single {
            it.kind ==
                PreparedProseFragmentKind.TEXT
        }
        assertEquals(7, text.bounds.left)
        assertEquals(13, text.bounds.top)
        assertEquals(171, text.layout!!.width)
        assertEquals(text.bounds.bottom + 15, result.heightPx)
    }
}
