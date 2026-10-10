#!/usr/bin/env python3
"""Apply the strict UTF-8 encoder optimization to UniFFI Kotlin output."""
import pathlib
import sys

ORIGINAL_ENCODER = """    fun toUtf8(value: String): ByteBuffer {
        // Make sure we don't have invalid UTF-16, check for lone surrogates.
        return Charsets.UTF_8.newEncoder().run {
            onMalformedInput(CodingErrorAction.REPORT)
            encode(CharBuffer.wrap(value))
        }
    }"""

OPTIMIZED_ENCODER = """    fun toUtf8(value: String): ByteBuffer {
        // String's native UTF-8 encoder replaces malformed input, so validate first.
        var index = 0
        while (index < value.length) {
            val unit = value[index]
            if (Character.isHighSurrogate(unit)) {
                index += 1
                if (index == value.length || !Character.isLowSurrogate(value[index])) {
                    throw java.nio.charset.MalformedInputException(1)
                }
            } else if (Character.isLowSurrogate(unit)) {
                throw java.nio.charset.MalformedInputException(1)
            }
            index += 1
        }
        return ByteBuffer.wrap(value.toByteArray(Charsets.UTF_8))
    }"""


def main():
    path = pathlib.Path(sys.argv[1])
    source = path.read_text()
    if source.count(ORIGINAL_ENCODER) != 1 or source.count("fun toUtf8(") != 1:
        raise SystemExit("Unexpected UniFFI Kotlin encoder; review the binding optimization before regenerating")
    path.write_text(source.replace(ORIGINAL_ENCODER, OPTIMIZED_ENCODER)
                    .replace("import java.nio.CharBuffer\n", "")
                    .replace("import java.nio.charset.CodingErrorAction\n", ""))


if __name__ == "__main__":
    main()
