package com.apollohg.editor.viewer

import com.apollohg.editor.tables.ViewerTablePresentedAccessibilityNode
import com.apollohg.editor.tables.TableAccessibilityNodes

internal class PreparedProseAccessibilityNodeRegistry {
    companion object {
        private const val FIRST_ANNOTATION_ID = 1
    }

    private class Entry(val parentId: Int?, val resolve: () -> ViewerTablePresentedAccessibilityNode?)

    private var nextId = FIRST_ANNOTATION_ID
    private val ids = mutableMapOf<String, Int>()
    private val entries = mutableMapOf<Int, Entry>()

    fun idOf(identity: String, parentId: Int?, resolve: () -> ViewerTablePresentedAccessibilityNode?): Int? {
        ids[identity]?.let { return it }
        if (nextId >= TableAccessibilityNodes.FIRST_TABLE_NODE_ID) return null
        val id = nextId++
        ids[identity] = id
        entries[id] = Entry(parentId, resolve)
        return id
    }

    fun node(id: Int): ViewerTablePresentedAccessibilityNode? = entries[id]?.resolve?.invoke()

    fun idOf(identity: String): Int? = ids[identity]

    fun parentId(id: Int): Int? = entries[id]?.parentId

    fun clear() {
        ids.clear()
        entries.clear()
    }
}

