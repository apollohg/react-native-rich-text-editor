package com.apollohg.editor

internal inline fun IntArray.partitionPoint(
    size: Int = this.size,
    isBefore: (index: Int) -> Boolean
): Int {
    var low = 0
    var high = size
    while (low < high) {
        val middle = (low + high) ushr 1
        if (isBefore(middle)) low = middle + 1 else high = middle
    }
    return low
}
