package com.apollohg.editor.tables

data class TableCellMeasurementKey(
    val documentOwner: String,
    val contentKey: String,
    val innerWidthPixels: Int,
    val themeDigest: String,
    val fontEnvironmentRevision: Long,
    val textScale: Float,
    val attachmentRevision: Long
)

class TableCellMeasurementCache(private val capacity: Int = 32768, private val byteLimit: Long = 32L * 1024L * 1024L) {
    private data class Entry(val value: Float, val bytes: Long)
    private val entries = LinkedHashMap<TableCellMeasurementKey, Entry>(16, 0.75f, true)
    var retainedBytes: Long = 0
        private set

    fun get(key: TableCellMeasurementKey): Float? = entries[key]?.value

    fun put(key: TableCellMeasurementKey, value: Float) {
        if (!value.isFinite() || value < 0f) return
        entries.remove(key)?.also { retainedBytes -= it.bytes }
        val bytes = maxOf(64L, key.documentOwner.length.toLong() * 2L + key.contentKey.length.toLong() * 2L + key.themeDigest.length.toLong() * 2L + 48L)
        entries[key] = Entry(value, bytes)
        retainedBytes += bytes
        while ((retainedBytes > byteLimit || entries.size > capacity) && entries.isNotEmpty()) {
            val oldest = entries.entries.iterator().next()
            retainedBytes -= oldest.value.bytes
            entries.remove(oldest.key)
        }
    }
}
