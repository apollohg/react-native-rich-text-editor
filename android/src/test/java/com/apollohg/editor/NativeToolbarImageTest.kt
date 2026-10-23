package com.apollohg.editor

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import com.facebook.common.references.CloseableReference
import com.facebook.datasource.SimpleDataSource
import com.facebook.imagepipeline.image.CloseableImage
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class NativeToolbarImageTest {
    @Test
    fun `image descriptors preserve URI and fallback with default tinting`() {
        val icon = requireNotNull(
            NativeToolbarIcon.fromJson(
                JSONObject(
                    """{"type":"image","source":{"uri":"toolbar_logo","width":48,"height":24,"scale":2},"fallbackText":"Logo"}"""
                )
            )
        )
        assertEquals("toolbar_logo", icon.imageSource?.uri)
        assertEquals(48.0, icon.imageSource?.width)
        assertEquals(24.0, icon.imageSource?.height)
        assertEquals(2.0, icon.imageSource?.scale)
        assertTrue(icon.tintable)
        assertEquals("Logo", icon.resolvedGlyphText())
        assertNull(
            NativeToolbarIcon.fromJson(JSONObject("""{"type":"image","source":{"uri":""}}"""))
        )
    }

    @Test
    fun `request resolves RN resources and URI sources and cancels late failure delivery`() {
        val context = RuntimeEnvironment.getApplication()
        val sources = mapOf(
            "abc_btn_default_mtrl_shape" to "res",
            "file:///tmp/icon.png" to "file",
            "/tmp/icon.png" to "file",
            "data:image/png;base64,AA==" to "data",
            "https://example.test/icon.png" to "https",
            "http://example.test/icon.png" to "http"
        )
        sources.forEach { (uri, scheme) ->
            val result = SimpleDataSource.create<CloseableReference<CloseableImage>>()
            var deliveries = 0
            val request = ToolbarImageRequest(
                context,
                NativeToolbarImageSource(uri),
                24,
                fetch = { imageRequest, _ ->
                    assertEquals("Resolved source for $uri", scheme, imageRequest.sourceUri.scheme)
                    result
                }
            ) { deliveries++ }
            request.close()
            assertTrue("Cancellation closes the pipeline subscription for $uri", result.isClosed)
            result.setFailure(java.io.IOException("Late load failure"))
            org.robolectric.Shadows.shadowOf(android.os.Looper.getMainLooper()).idle()
            assertEquals("A cancelled request must not update its old button", 0, deliveries)
        }
    }

    @Test
    fun `menu load failures show explicit or default fallback without losing the label`() {
        val context = RuntimeEnvironment.getApplication()
        val toolbar = EditorKeyboardToolbarView(context)
        val menu = androidx.appcompat.view.menu.MenuBuilder(context)
        listOf("Logo" to "Logo Insert", null to "? Insert").forEach { (fallback, expected) ->
            val child = NativeToolbarItem(
                ToolbarItemKind.ACTION,
                label = "Insert",
                icon = NativeToolbarIcon(
                    imageSource = NativeToolbarImageSource("missing"),
                    fallbackText = fallback
                )
            )
            val row = menu.add("Insert")
            toolbar.updateMenuItemImage(row, child, null, enabled = true, active = false)
            assertEquals(expected, row.title.toString())
            assertNull(row.icon)
        }
    }

    @Test
    fun `menu artwork respects item and theme size precedence and row size cap`() {
        val context = RuntimeEnvironment.getApplication()
        val toolbar = EditorKeyboardToolbarView(context)
        val menu = androidx.appcompat.view.menu.MenuBuilder(context)
        val child = NativeToolbarItem(
            ToolbarItemKind.ACTION,
            label = "Insert",
            icon = NativeToolbarIcon(imageSource = NativeToolbarImageSource("missing"))
        )
        val row = menu.add("Insert")
        val scale = context.resources.displayMetrics.scaledDensity
        fun checkSize(themeSize: Float?, itemSize: Float?, expected: Float) {
            toolbar.applyTheme(EditorToolbarTheme(buttonIconSize = themeSize))
            toolbar.updateMenuItemImage(
                row,
                child.copy(buttonStyle = EditorToolbarButtonStyle(iconSize = itemSize)),
                ColorDrawable(Color.BLUE),
                enabled = true,
                active = false
            )
            assertEquals(
                "Icon width for theme=$themeSize item=$itemSize",
                (expected * scale).toInt(),
                requireNotNull(row.icon).bounds.width()
            )
            assertEquals(
                "Menu ImageView must measure the requested icon size",
                (expected * scale).toInt(),
                requireNotNull(row.icon).intrinsicWidth
            )
        }
        checkSize(null, null, 24f)
        checkSize(30f, null, 30f)
        checkSize(30f, 18f, 18f)
        checkSize(100f, null, 40f)
        checkSize(30f, 100f, 40f)
        val bitmap = Bitmap.createBitmap(12, 6, Bitmap.Config.ARGB_8888)
        toolbar.updateMenuItemImage(
            row,
            child,
            android.graphics.drawable.BitmapDrawable(context.resources, bitmap),
            enabled = true,
            active = false
        )
        assertEquals(
            "Menu layout preserves rectangular artwork aspect ratio",
            requireNotNull(row.icon).intrinsicWidth / 2,
            requireNotNull(row.icon).intrinsicHeight
        )
        bitmap.recycle()
    }

    @Test
    fun `React glyph lookup registers the bundled font and rejects unknown names`() {
        val context = RuntimeEnvironment.getApplication()
        val glyph = requireNotNull(MaterialIconRegistry.reactIcon(context, "format-bold"))
        assertEquals(MaterialIconRegistry.glyphForName(context, "format-bold"), glyph["glyph"])
        assertEquals("NativeEditorMaterialDesignIcons", glyph["fontFamily"])
        assertSame(
            MaterialIconRegistry.typeface(context),
            com.facebook.react.common.assets.ReactFontManager.getInstance().getTypeface(
                requireNotNull(glyph["fontFamily"]),
                android.graphics.Typeface.NORMAL,
                context.assets
            )
        )
        assertNull(MaterialIconRegistry.reactIcon(context, "not-an-editor-icon"))
        assertNull(MaterialIconRegistry.reactIcon(context, ""))
    }

    @Test
    @org.robolectric.annotation.GraphicsMode(org.robolectric.annotation.GraphicsMode.Mode.NATIVE)
    fun `image buttons preserve fallback and update tint without changing full colour artwork`() {
        val context = RuntimeEnvironment.getApplication()
        val button = ToolbarImageButton(context)
        button.text = "Fallback"
        button.setTextColor(Color.RED)
        val image = ColorDrawable(Color.BLUE)
        button.imageTintable = true
        button.imageDrawable = image
        button.layout(0, 0, 80, 80)
        fun renderedCentre(): Int {
            val bitmap = Bitmap.createBitmap(80, 80, Bitmap.Config.ARGB_8888)
            button.draw(Canvas(bitmap))
            return bitmap.getPixel(40, 40).also { bitmap.recycle() }
        }
        assertEquals("Tinted artwork uses the current button colour", Color.RED, renderedCentre())
        button.setTextColor(Color.GREEN)
        assertEquals("State changes retint loaded artwork", Color.GREEN, renderedCentre())
        button.imageTintable = false
        assertEquals(
            "Full colour artwork retains its original pixels",
            Color.BLUE,
            renderedCentre()
        )
        button.imageDrawable = null
        assertEquals("Load failure preserves fallback text", "Fallback", button.text.toString())
    }
}
