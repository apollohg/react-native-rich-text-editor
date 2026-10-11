package com.apollohg.editor

import android.app.Activity
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import com.apollohg.editor.tables.PlainTableFixture
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.facebook.react.internal.featureflags.ReactNativeFeatureFlagsForTests
import com.facebook.react.views.view.ReactViewGroup
import java.time.Duration
import org.json.JSONObject
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class NativeEditorTableScrollLayoutTest {
    @Test
    fun `table content height reaches scroll container inside React Native`() {
        ReactNativeFeatureFlagsForTests.setUp()
        val activity = Robolectric.buildActivity(Activity::class.java).setup()
        val created = UniffiEditorV2Backend.create(
            PlainTableFixture.CONFIG,
            null
        ) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        try {
            requireNotNull(
                adapter.setContentJson(PlainTableFixture.document(TABLE_ROWS, TABLE_COLUMNS))
            )
            val expo = testExpoContext(activity.get())
            val wrapper = NativeEditorExpoView(expo.context, expo.appContext)
            wrapper.onAddonEventForTesting = {}
            wrapper.onFocusChangeForTesting = {}
            wrapper.onEditorReadyForTesting = {}
            wrapper.onTableSelectionGeometryForTesting = {}
            wrapper.onEditorUpdateForTesting = {}
            wrapper.onSelectionChangeForTesting = {}
            wrapper.onContentHeightChangeForTesting = {}
            val parent = ReactViewGroup(expo.context)
            activity.get().setContentView(parent)
            parent.addView(wrapper, ViewGroup.LayoutParams(HOST_WIDTH, HOST_HEIGHT))
            wrapper.setEditorId(token)
            wrapper.measure(
                View.MeasureSpec.makeMeasureSpec(HOST_WIDTH, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(HOST_HEIGHT, View.MeasureSpec.EXACTLY)
            )
            wrapper.layout(0, 0, HOST_WIDTH, HOST_HEIGHT)
            shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(1))
            val editor = wrapper.richTextView
            val canvas = (0 until editor.editorContentFrame.childCount)
                .map(editor.editorContentFrame::getChildAt)
                .filterIsInstance<PreparedProseDrawingView>().single()
            val bottom = requireNotNull(canvas.preparedLayout).blocks.single().tableBounds!!.bottom
            assertTrue(
                "table bottom=$bottom must exceed viewport=${editor.editorScrollView.height}",
                bottom > editor.editorScrollView.height
            )
            assertTrue(
                "content=${editor.editorContentFrame.height} must contain table bottom=$bottom; text layout=${editor.editorEditText.layout.height}",
                editor.editorContentFrame.height >= bottom
            )
            assertTrue(
                "table must supply vertical scroll range",
                editor.editorScrollView.canScrollVertically(1)
            )
        } finally {
            activity.pause().stop().destroy()
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private companion object {
        const val HOST_WIDTH = 600
        const val HOST_HEIGHT = 500
        const val TABLE_ROWS = 40
        const val TABLE_COLUMNS = 2
    }
}
