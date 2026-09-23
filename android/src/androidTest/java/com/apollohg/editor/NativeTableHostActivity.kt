package com.apollohg.editor

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject

class NativeTableHostActivity : Activity() {
    internal lateinit var richTextView: RichTextEditorView
    internal lateinit var adapter: EditorV2Adapter
    internal lateinit var documentBeforeMount: String
    internal lateinit var historyBeforeMount: Pair<Boolean?, Boolean?>
    internal var revisionBeforeMount = 0uL
    private var viewToken = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val dark = intent.getBooleanExtra(EXTRA_DARK, false)
        val background = if (dark) Color.rgb(19, 27, 35) else Color.WHITE
        val foreground = if (dark) Color.WHITE else Color.rgb(29, 48, 58)
        window.statusBarColor = background
        window.navigationBarColor = background

        val created = when (val result = UniffiEditorV2Backend.create(CONFIG, null)) {
            is EditorV2CallResult.Ok -> result.value
            is EditorV2CallResult.Err -> error("Table fixture create failed: ${result.error.code}: ${result.error.message}")
        }
        val id = JSONObject(created).getString("editorId")
        adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, id, roomBound = false))
        requireNotNull(adapter.setContentJson(DOCUMENT))
        documentBeforeMount = requireNotNull(adapter.documentJson())
        historyBeforeMount = adapter.historyCanUndo() to adapter.historyCanRedo()
        revisionBeforeMount = adapter.baseDocumentRevision
        viewToken = EditorV2Registry.register(adapter)

        richTextView = RichTextEditorView(this).apply {
            applyTheme(EditorTheme.fromJson(themeJson(dark)))
            editorId = viewToken
        }
        val padding = (16 * resources.displayMetrics.density).toInt()
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(background)
            addView(TextView(this@NativeTableHostActivity).apply {
                text = "Native table host · ${if (dark) "dark" else "light"}"
                textSize = 18f
                setTextColor(foreground)
                setPadding(0, 0, 0, padding)
            })
            addView(richTextView, LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f
            ))
        }
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            view.setPadding(
                padding + bars.left,
                padding + bars.top,
                padding + bars.right,
                padding + bars.bottom
            )
            insets
        }
        setContentView(root)
        ViewCompat.requestApplyInsets(root)
    }

    override fun onDestroy() {
        if (::richTextView.isInitialized) richTextView.editorId = 0L
        if (viewToken != 0L) releasePairedV2TestEditor(viewToken)
        super.onDestroy()
    }

    private fun themeJson(dark: Boolean): String = if (dark) {
        """{"text":{"fontSize":18,"color":"#ffffffff"},"backgroundColor":"#131b23ff","contentInsets":{"top":12,"right":12,"bottom":12,"left":12},"table":{"borderColor":"#71808fff","headerBackgroundColor":"#334252ff","minColumnWidth":72,"cellPadding":8}}"""
    } else {
        """{"text":{"fontSize":18,"color":"#1d303aff"},"backgroundColor":"#ffffffff","contentInsets":{"top":12,"right":12,"bottom":12,"left":12},"table":{"borderColor":"#a0acb7ff","headerBackgroundColor":"#e8f0f5ff","minColumnWidth":72,"cellPadding":8}}"""
    }

    companion object {
        const val EXTRA_DARK = "dark"

        private const val CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"""

        private const val DOCUMENT = """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Before table."}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Project"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"Status"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Alpha"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Owner"}]}]},{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Ready"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Beta"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"After table."}]}]}"""
    }
}
