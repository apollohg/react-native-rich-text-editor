package com.apollohg.editor.tables

import com.apollohg.editor.viewer.PREPARED_LAYOUT_UNMOUNTED_BYTE_BUDGET
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.cellShapeCatalogBytes

internal class TableCellLayoutStore(
    private val byteBudget: Long = PREPARED_LAYOUT_UNMOUNTED_BYTE_BUDGET,
    private val capacity: Int = MAXIMUM_RESIDENT_LAYOUTS
) {
    private class Entry(val layout: PreparedProseLayout) {
        val bytes = layout.retainedBytes + layout.cellShapeCatalogBytes()
    }

    private val entries = linkedMapOf<ProseLayoutKey, Entry>()
    private val pins = mutableMapOf<ProseLayoutKey, Int>()
    private var bytes = 0L
    private var pinnedBytes = 0L

    val count: Int @Synchronized get() = entries.size
    val unmountedRetainedBytes: Long @Synchronized get() = bytes - pinnedBytes

    @Synchronized fun peek(key: ProseLayoutKey): PreparedProseLayout? = entries[key]?.layout

    @Synchronized fun value(key: ProseLayoutKey, build: () -> PreparedProseLayout): PreparedProseLayout {
        entries.remove(key)?.let { entry ->
            entries[key] = entry
            return entry.layout
        }
        return build().also { insert(it, key) }
    }

    @Synchronized fun insert(layout: PreparedProseLayout, key: ProseLayoutKey = layout.key) {
        entries.remove(key)?.let { removeCharge(key, it) }
        val entry = Entry(layout)
        entries[key] = entry
        bytes += entry.bytes
        if (pins.getOrDefault(key, 0) > 0) pinnedBytes += entry.bytes
        evict()
    }

    @Synchronized fun pin(key: ProseLayoutKey) {
        if (pins.getOrDefault(key, 0) == 0) pinnedBytes += entries[key]?.bytes ?: 0L
        pins[key] = pins.getOrDefault(key, 0) + 1
    }

    @Synchronized fun unpin(key: ProseLayoutKey) {
        val count = pins[key] ?: return
        if (count > 1) { pins[key] = count - 1; return }
        pins.remove(key)
        pinnedBytes -= entries[key]?.bytes ?: 0L
        evict()
    }

    private fun evict() {
        val iterator = entries.iterator()
        while ((bytes - pinnedBytes > byteBudget || entries.size > capacity) && iterator.hasNext()) {
            val entry = iterator.next()
            if (pins.getOrDefault(entry.key, 0) == 0) {
                removeCharge(entry.key, entry.value)
                iterator.remove()
            }
        }
    }

    private fun removeCharge(key: ProseLayoutKey, entry: Entry) {
        bytes -= entry.bytes
        if (pins.getOrDefault(key, 0) > 0) pinnedBytes -= entry.bytes
    }

    companion object {
        const val MAXIMUM_RESIDENT_LAYOUTS = 2_272
    }
}
