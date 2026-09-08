package com.apollohg.editor

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorStyleSheetCoreAdmissionTest {
    @Test
    fun `real core language and mention style survive adapter admission`() {
        val config =
            """{"initialization":{"type":"localEmpty"}""" +
                ""","schema":{"nodes":[{"name":"doc","content":"block""" +
                """+","role":"doc"},{"name":"paragraph","content":"inline""" +
                """*","group":"block","role":"textBlock"},{"name":"codeBloc""" +
                """k","content":"text*","group":"block","role":"textBloc""" +
                """k","attrs":{"language":{"default":null}}},{"name":"mentio""" +
                """n","group":"inline","isVoid":true,"content":"","role":"inlin""" +
                """e","attrs":{"id":{"default":""},"label":{"default":""}""" +
                ""","mentionTheme":{"default":null}}},{"name":"tex""" +
                """t","group":"inline","role":"text"}]}}"""
        val created = UniffiEditorV2Backend.create(config, null)
        assertTrue(created.toString(), created is EditorV2CallResult.Ok)
        val id = JSONObject((created as EditorV2CallResult.Ok).value).getString("editorId")
        val adapter = requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend, id, false))
        try {
            val update = adapter.setContentJson(

                """{"type":"doc","content":[{"type":"codeBloc""" +
                    """k","attrs":{"language":"rust"},"content":[{"type":"tex""" +
                    """t","text":"let x = 1;"}]},{"type":"paragrap""" +
                    """h","content":[{"type":"mention","attrs":{"id":"ad""" +
                    """a","label":"Ada","mentionTheme":{"node":{"style":{"color":"#12345""" +
                    """6ff","borderLeftWidth":2,"fontWeight":"700","paddingTop":0,"paddingRight":8,"paddingBottom":3,"paddingLeft":0}}}}}]}]}"""
            )
            assertNotNull(update)
            val blocks = JSONObject(requireNotNull(update)).getJSONArray("renderBlocks")
            assertEquals("rust", blocks.getJSONArray(0).getJSONObject(0).getString("language"))
            val mention = blocks.getJSONArray(1).getJSONObject(1)
            assertEquals(
                "#123456ff",
                mention.getJSONObject(
                    "mentionTheme"
                ).getJSONObject("node").getJSONObject("style").getString("color")
            )
            val style = mention.getJSONObject("mentionTheme").getJSONObject("node")
                .getJSONObject("style")
            assertEquals(0, style.getInt("paddingTop"))
            assertEquals(8, style.getInt("paddingRight"))
            assertEquals(3, style.getInt("paddingBottom"))
            assertEquals(0, style.getInt("paddingLeft"))
        } finally {
            adapter.destroy()
        }
    }
}
