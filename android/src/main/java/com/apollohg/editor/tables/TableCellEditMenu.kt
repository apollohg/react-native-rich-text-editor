package com.apollohg.editor.tables

import android.graphics.Rect
import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.view.ViewTreeObserver
import com.apollohg.editor.EditorEditText
import com.apollohg.editor.canPerformCellSelectionMenuItem

internal class TableCellEditMenu(
    private val root: EditorEditText,
    private val anchor: () -> Rect?,
    private val visibilityChanged: () -> Unit,
    private val tableActions: () -> Map<TableAccessibilityAction, () -> Boolean>
) {
    private companion object {
        val ITEMS = listOf(
            android.R.id.cut to android.R.string.cut,
            android.R.id.copy to android.R.string.copy,
            android.R.id.paste to android.R.string.paste
        )
    }

    private var mode: ActionMode? = null
    private var actions: Map<TableAccessibilityAction, () -> Boolean> = emptyMap()
    private var observedTree: ViewTreeObserver? = null
    private val scrollListener = ViewTreeObserver.OnScrollChangedListener { reanchor() }

    val isVisible: Boolean get() = mode != null

    private val callback = object : ActionMode.Callback2() {
        override fun onCreateActionMode(mode: ActionMode, menu: Menu): Boolean {
            ITEMS.forEachIndexed { order, (id, title) ->
                if (root.canPerformCellSelectionMenuItem(id)) {
                    menu.add(
                        Menu.NONE,
                        id,
                        order,
                        title
                    ).setShowAsAction(MenuItem.SHOW_AS_ACTION_IF_ROOM)
                }
            }
            actions = tableActions()
            actions.keys.forEach { action ->
                menu.add(Menu.NONE, action.id, Menu.NONE, action.label)
                    .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER)
            }
            return menu.size() > 0
        }

        override fun onPrepareActionMode(mode: ActionMode, menu: Menu) = false

        override fun onActionItemClicked(mode: ActionMode, item: MenuItem): Boolean {
            if (this@TableCellEditMenu.mode !== mode) return false
            val action = actions.entries.firstOrNull { it.key.id == item.itemId }?.value
            val handled = action?.invoke() ?: root.onTextContextMenuItem(item.itemId)
            if (this@TableCellEditMenu.mode === mode) mode.finish()
            return handled
        }

        override fun onDestroyActionMode(mode: ActionMode) {
            if (this@TableCellEditMenu.mode !== mode) return
            this@TableCellEditMenu.mode = null
            actions = emptyMap()
            if (root.selectionActionMode === mode) root.selectionActionMode = null
            observedTree?.takeIf { it.isAlive }?.removeOnScrollChangedListener(scrollListener)
            observedTree = null
            visibilityChanged()
        }

        override fun onGetContentRect(mode: ActionMode, view: View, outRect: Rect) {
            anchor()?.let(outRect::set)
        }
    }

    fun present() {
        if (mode != null || anchor() == null) return
        root.selectionActionMode?.finish()
        val started = root.startActionMode(callback, ActionMode.TYPE_FLOATING) ?: return
        mode = started
        root.selectionActionMode = started
        observedTree = root.viewTreeObserver.also { it.addOnScrollChangedListener(scrollListener) }
        visibilityChanged()
    }

    fun dismiss() {
        mode?.finish()
    }

    fun reanchor() {
        val current = mode ?: return
        if (anchor() == null) current.finish() else current.invalidateContentRect()
    }
}
