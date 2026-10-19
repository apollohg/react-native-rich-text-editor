package com.apollohg.editor

private const val MANAGEMENT_FACTORY_CLASS = "java.lang.management.ManagementFactory"
private const val THREAD_BEAN_CLASS = "com.sun.management.ThreadMXBean"

internal fun bytesAllocatedBy(block: () -> Unit): Long {
    val bean = Class.forName(MANAGEMENT_FACTORY_CLASS).getMethod("getThreadMXBean").invoke(null)
    val allocated = Class.forName(THREAD_BEAN_CLASS).getMethod("getCurrentThreadAllocatedBytes")
    val before = allocated.invoke(bean) as Long
    block()
    return allocated.invoke(bean) as Long - before
}
