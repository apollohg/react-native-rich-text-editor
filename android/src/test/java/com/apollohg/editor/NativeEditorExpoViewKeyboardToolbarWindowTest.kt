package com.apollohg.editor

import android.app.Dialog
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class NativeEditorExpoViewKeyboardToolbarWindowTest :
    NativeEditorExpoViewControlledUpdateTestFixture() {
    @Test
    fun `dialog keyboard toolbar attaches above modal content and receives its IME insets`() {
        withDialogEditor { activityHost, dialogHost, view, _ ->
            val toolbar = view.keyboardToolbarView
            assertSame(
                "the toolbar must share the editor's dialog window",
                dialogHost,
                toolbar.parent
            )
            assertSame(
                "the toolbar is above modal content",
                toolbar,
                dialogHost.getChildAt(dialogHost.childCount - 1)
            )

            ViewCompat.dispatchApplyWindowInsets(activityHost, imeInsets(ACTIVITY_IME_BOTTOM_PX))
            ViewCompat.dispatchApplyWindowInsets(dialogHost, imeInsets(DIALOG_IME_BOTTOM_PX))

            assertEquals(
                "the toolbar receives the dialog's IME height",
                DIALOG_IME_BOTTOM_PX,
                view.currentImeBottom
            )
            assertEquals(
                "the toolbar is visible above the keyboard",
                View.VISIBLE,
                toolbar.visibility
            )
            assertEquals(
                "the toolbar is placed above the dialog's keyboard",
                DIALOG_IME_BOTTOM_PX,
                (toolbar.layoutParams as FrameLayout.LayoutParams).bottomMargin
            )
            assertTrue(
                "toolbar attachment preserves editor focus",
                view.richTextView.activeTextInput.hasFocus()
            )
        }
    }

    @Test
    fun `dialog toolbar measurement and caret clearance use dialog bounds`() {
        withDialogEditor { activityHost, dialogHost, view, _ ->
            activityHost.layout(0, 0, ACTIVITY_WIDTH_PX, ACTIVITY_HEIGHT_PX)
            dialogHost.layout(0, 0, DIALOG_WIDTH_PX, DIALOG_HEIGHT_PX)
            view.currentImeBottom = DIALOG_IME_BOTTOM_PX
            view.updateEditorViewportInset(forceMeasureToolbar = true)

            assertEquals(
                "toolbar width is measured against the dialog",
                DIALOG_WIDTH_PX,
                view.keyboardToolbarView.measuredWidth
            )
            val hostLocation = IntArray(2).also(dialogHost::getLocationOnScreen)
            val toolbarHeight = view.keyboardToolbarView.measuredHeight
            assertEquals(
                "caret clearance must use the dialog's keyboard edge",
                hostLocation[1] + DIALOG_HEIGHT_PX - DIALOG_IME_BOTTOM_PX - toolbarHeight,
                view.resolveToolbarTopOnScreenPx(toolbarHeight, keyboardOffsetPx = 0)
            )
        }
    }

    @Test
    fun `dismissing dialog removes keyboard toolbar and clears its viewport inset`() {
        withDialogEditor { _, _, view, dialog ->
            ViewCompat.dispatchApplyWindowInsets(
                view.keyboardToolbarView,
                imeInsets(DIALOG_IME_BOTTOM_PX)
            )
            assertTrue(
                "the visible toolbar reserves caret clearance",
                view.richTextView.viewportBottomInsetPxForTesting() > 0
            )

            dialog.dismiss()
            shadowOf(Looper.getMainLooper()).idle()

            assertNull("dialog dismissal removes its toolbar", view.keyboardToolbarView.parent)
            assertEquals("dialog dismissal clears the keyboard height", 0, view.currentImeBottom)
            assertEquals(
                "dialog dismissal clears caret clearance",
                0,
                view.richTextView.viewportBottomInsetPxForTesting()
            )
        }
    }

    private fun withDialogEditor(
        block: (FrameLayout, FrameLayout, NativeEditorExpoView, Dialog) -> Unit
    ) {
        withFocusedViewForNativeInput { activity, view, _, _ ->
            val activityHost = activity.findViewById<FrameLayout>(android.R.id.content)
            val dialog = Dialog(activity)
            val modalContent = FrameLayout(activity)
            val toolbar = view.keyboardToolbarView
            try {
                (view.parent as ViewGroup).removeView(view)
                modalContent.addView(view)
                dialog.setContentView(modalContent)
                dialog.show()
                shadowOf(Looper.getMainLooper()).idle()
                assertTrue(
                    "the editor focuses in the dialog",
                    view.richTextView.activeTextInput.requestFocus()
                )
                view.updateKeyboardToolbarVisibility()
                val dialogHost = dialog.findViewById<FrameLayout>(android.R.id.content)
                assertSame(
                    "the editor belongs to the dialog root",
                    dialogHost.rootView,
                    view.rootView
                )
                assertSame(
                    "the Expo context still resolves the activity",
                    activity,
                    view.resolveActivity(view.context)
                )
                block(activityHost, dialogHost, view, dialog)
            } finally {
                dialog.dismiss()
                shadowOf(Looper.getMainLooper()).idle()
                (toolbar.parent as? ViewGroup)?.removeView(toolbar)
            }
        }
    }

    private fun imeInsets(bottom: Int): WindowInsetsCompat = WindowInsetsCompat.Builder()
        .setInsets(WindowInsetsCompat.Type.ime(), Insets.of(0, 0, 0, bottom))
        .build()

    private companion object {
        const val ACTIVITY_WIDTH_PX = 1080
        const val ACTIVITY_HEIGHT_PX = 1800
        const val DIALOG_WIDTH_PX = 320
        const val DIALOG_HEIGHT_PX = 1000
        const val ACTIVITY_IME_BOTTOM_PX = 0
        const val DIALOG_IME_BOTTOM_PX = 300
    }
}
