package com.apollohg.editor

import java.nio.ByteBuffer
import java.nio.CharBuffer
import java.nio.charset.CodingErrorAction
import java.nio.charset.MalformedInputException
import kotlin.random.Random
import org.junit.Assert.*
import org.junit.Test
import uniffi.editor_core.FfiConverterString

class UniffiStringEncodingTest {
    private fun reference(value: String): ByteBuffer = Charsets.UTF_8.newEncoder()
        .onMalformedInput(CodingErrorAction.REPORT).encode(CharBuffer.wrap(value))

    private fun verify(value: String) {
        val expected = try {
            reference(value)
        } catch (error: MalformedInputException) {
            val actual = assertThrows(MalformedInputException::class.java) {
                FfiConverterString.toUtf8(value)
            }
            assertEquals(error.inputLength, actual.inputLength)
            val destination = ByteBuffer.allocate(value.length * MAX_UTF8_BYTES_PER_UNIT + LENGTH_BYTES)
            assertThrows(MalformedInputException::class.java) { FfiConverterString.write(value, destination) }
            assertEquals("Malformed input must not partially write a record", 0, destination.position())
            return
        }
        assertEquals(expected, FfiConverterString.toUtf8(value))
        val destination = ByteBuffer.allocate(expected.remaining() + LENGTH_BYTES + SENTINEL_BYTES)
        destination.put(SENTINEL)
        FfiConverterString.write(value, destination)
        destination.put(SENTINEL)
        destination.flip()
        assertEquals(SENTINEL, destination.get())
        assertEquals(value, FfiConverterString.read(destination))
        assertEquals(SENTINEL, destination.get())
        assertFalse(destination.hasRemaining())
    }

    @Test fun everyUtf16UnitMatchesTheStrictEncoder() {
        for (unit in Char.MIN_VALUE.code..Char.MAX_VALUE.code) verify(unit.toChar().toString())
    }

    @Test fun surrogatePairsAndMalformedSequencesMatchTheStrictEncoder() {
        for (high in Char.MIN_HIGH_SURROGATE.code..Char.MAX_HIGH_SURROGATE.code) {
            for (low in listOf(Char.MIN_LOW_SURROGATE, Char.MAX_LOW_SURROGATE)) {
                val pair = "${high.toChar()}$low"
                verify(pair)
                verify("a${pair}é\u0000中")
                verify("$pair${high.toChar()}")
                verify("${low}$pair")
                verify("${high.toChar()}$pair")
            }
        }
        val random = Random(RANDOM_SEED)
        repeat(RANDOM_CASES) {
            verify(CharArray(random.nextInt(MAX_RANDOM_LENGTH)) {
                random.nextInt(Char.MAX_VALUE.code + 1).toChar()
            }.concatToString())
        }
        verify("")
        verify("a😀e\u0301 العربية\n中\u0000".repeat(LONG_TEXT_REPEATS))
    }

    private companion object {
        const val MAX_UTF8_BYTES_PER_UNIT = 3
        const val LENGTH_BYTES = Int.SIZE_BYTES
        const val SENTINEL_BYTES = 2
        const val SENTINEL: Byte = 42
        const val RANDOM_SEED = 714
        const val RANDOM_CASES = 2_000
        const val MAX_RANDOM_LENGTH = 128
        const val LONG_TEXT_REPEATS = 2_000
    }
}
