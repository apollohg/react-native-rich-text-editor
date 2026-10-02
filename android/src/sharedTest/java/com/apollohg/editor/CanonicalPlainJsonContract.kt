package com.apollohg.editor

import kotlin.random.Random
import org.json.JSONException
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

abstract class CanonicalPlainJsonContract {
    @Test fun compactPlainDocumentsAreAlreadyNormalized() {
        val documents = listOf(
            "{}", """{"content":[]}""", """{"type":"doc","content":[]}""",
            """{"content":[{"text":"a b!~","type":"text"}],"type":"doc"}""",
            """{"type":"unknown","content":[{"type":"paragraph","content":[{"type":"text","text":"hello"}]}]}"""
        )
        for (source in documents) {
            assertTrue("Expected a normalization certificate: $source", CanonicalPlainJson.accepts(source))
            assertEquals("Certificate must preserve every byte", source, JSONObject(source).toString())
        }
    }

    @Test fun unsupportedSyntaxNeverReceivesACertificate() {
        for (source in listOf(
            "", "[]", "null", " {}", "{} ", "{}ignored", "{ }",
            """{"type": "doc"}""", """{"type":"doc","type":"text"}""",
            """{"type":"text","text":"a/b"}""", """{"type":"text","text":"a\nb"}""",
            """{"type":"text","text":"é"}""", """{"type":"text","text":null}""",
            """{"attrs":{"x":1},"type":"doc"}""", """{"content":[1]}""",
            """{"content":{}}""", """{"type":true}""", """{"type":"doc",}""",
            """{"content":[{},]}""", """{'type':'doc'}""", """{type:doc}""",
            """{"type":"doc"""", """{"content":[{}]"""
        )) assertFalse("Unsupported source was certified: $source", CanonicalPlainJson.accepts(source))
    }

    private fun referencePayload(source: String): String = JSONObject()
        .put("setJson", JSONObject(source)).put("history", "resetAndClear").toString()

    private fun assertPayloadParity(source: String) {
        val expected = try { referencePayload(source) } catch (error: JSONException) {
            val actual = assertThrows(JSONException::class.java) { prepareJsonReplacementPayload(source) }
            assertEquals("Parser error changed for $source", error.message, actual.message)
            return
        }
        assertEquals("Payload changed for $source", expected, prepareJsonReplacementPayload(source)())
        if (CanonicalPlainJson.accepts(source)) {
            assertEquals("Invalid certificate for $source", source, JSONObject(source).toString())
        }
    }

    @Test fun everyUtf16UnitAndLenientSourcesKeepExactPayloads() {
        for (unit in Char.MIN_VALUE.code..Char.MAX_VALUE.code) {
            assertPayloadParity("{\"type\":\"text\",\"text\":\"${unit.toChar()}\"}")
        }
        for (source in listOf(
            "{type:doc,content:[]}", "{'type':'doc'}", "{} ignored", "/*comment*/ {}", " { } ",
            "{\"type\":\"first\",\"type\":\"last\"}",
            "{\"type\":\"text\",\"text\":\"a\\/b\\u0063\"}",
            "{\"attrs\":{\"negativeZero\":-0,\"scientific\":1e3,\"hex\":0x10,\"octal\":010}}",
            "{\"content\":[{},]}", "{\"content\":[;]}", "{\"type\":true}", "{", "[]", "null"
        )) assertPayloadParity(source)
    }

    @Test fun mutationsAndDepthExhaustionAlwaysPreserveTheNormalizer() {
        val seeds = listOf("{}", "{\"type\":\"doc\",\"content\":[{\"type\":\"text\",\"text\":\"abc\"}]}",
            "{\"content\":[{},{},{}],\"type\":\"doc\"}")
        val mutations = "{}[],:\" /\\\n\t0af"
        val random = Random(RANDOM_SEED)
        repeat(MUTATION_CASES) {
            val original = seeds[random.nextInt(seeds.size)]
            val at = random.nextInt(original.length + 1)
            val character = mutations[random.nextInt(mutations.length)]
            assertPayloadParity(original.substring(0, at) + character + original.substring(at))
            if (at < original.length) {
                assertPayloadParity(original.removeRange(at, at + 1))
                assertPayloadParity(original.replaceRange(at, at + 1, character.toString()))
            }
        }
        val deep = "{\"content\":[".repeat(DEEP_CONTAINERS) + "{}" + "]}".repeat(DEEP_CONTAINERS)
        assertFalse("Depth exhaustion must choose the old parser", CanonicalPlainJson.accepts(deep))
        assertPayloadParity(deep)
        val excessive = "{\"content\":[".repeat(EXCESSIVE_CONTAINERS) + "{}" + "]}".repeat(EXCESSIVE_CONTAINERS)
        assertFalse("Certification must have bounded stack use", CanonicalPlainJson.accepts(excessive))
    }

    @Test fun largePlainTableUsesTheSamePayloadWithoutNormalization() {
        val fixture = com.apollohg.editor.tables.PlainTableFixture
        val source = fixture.document(fixture.LARGE_ROWS, fixture.LARGE_COLUMNS)
        assertTrue("The ordinary 20,000-cell document must be certifiable", CanonicalPlainJson.accepts(source))
        assertPayloadParity(source)
    }

    private companion object {
        const val RANDOM_SEED = 718
        const val MUTATION_CASES = 2_000
        const val DEEP_CONTAINERS = 192
        const val EXCESSIVE_CONTAINERS = 10_000
    }
}
