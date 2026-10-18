package com.apollohg.editor

import org.json.JSONObject

internal const val HARD_MAXIMUM_DOCUMENT_DEPTH = 1_024
private const val DOCUMENT_AND_LEAF_DEPTH = 2
private const val NODES_PER_NESTED_TABLE = 3
internal const val ADMITTED_NESTED_TABLE_DEPTH =
    (HARD_MAXIMUM_DOCUMENT_DEPTH - DOCUMENT_AND_LEAF_DEPTH) / NODES_PER_NESTED_TABLE
internal const val BOUNDED_THREAD_STACK_BYTES = 256L * 1_024L

internal fun nestedTablesSource(
    depth: Int,
    leaf: String = """{"type":"paragraph","content":[{"type":"text","text":"deep"}]}"""
): String {
    var node = leaf
    repeat(depth) {
        node =
            """{"type":"table","content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","content":[$node]}]}]}"""
    }
    return """{"type":"doc","content":[$node]}"""
}

internal fun configWithMaxDocumentDepth(config: String, maximum: Int): String =
    JSONObject(config).put(
        "limits",
        JSONObject().put("resource", JSONObject().put("maxDocumentDepth", maximum))
    ).toString()

internal fun runOnThreadStack(stackBytes: Long, body: () -> Unit) {
    var failure: Throwable? = null
    val thread = Thread(
        null,
        {
            try {
                body()
            } catch (error: Throwable) {
                failure = error
            }
        },
        "bounded-stack",
        stackBytes
    )
    thread.start()
    thread.join()
    failure?.let { throw it }
}
