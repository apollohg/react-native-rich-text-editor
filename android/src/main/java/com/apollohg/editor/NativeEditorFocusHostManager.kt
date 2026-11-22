package com.apollohg.editor

import android.view.View
import android.view.ViewGroup
import com.facebook.react.module.annotations.ReactModule
import com.facebook.react.uimanager.SimpleViewManager
import com.facebook.react.uimanager.ThemedReactContext
import com.facebook.react.uimanager.ViewManagerDelegate
import com.facebook.react.viewmanagers.NativeEditorFocusHostManagerDelegate
import com.facebook.react.viewmanagers.NativeEditorFocusHostManagerInterface

@ReactModule(name = NativeEditorFocusHostManager.REACT_CLASS)
internal class NativeEditorFocusHostManager :
    SimpleViewManager<View>(),
    NativeEditorFocusHostManagerInterface<View> {
    private val delegate: ViewManagerDelegate<View> = NativeEditorFocusHostManagerDelegate(this)

    override fun getName(): String = REACT_CLASS

    override fun getDelegate(): ViewManagerDelegate<View> = delegate

    override fun createViewInstance(context: ThemedReactContext): View = View(context)

    override fun focus(view: View) = Unit

    override fun blur(view: View) {
        val parent = view.parent as? ViewGroup ?: return
        (0 until parent.childCount)
            .firstNotNullOfOrNull { parent.getChildAt(it) as? NativeEditorExpoView }
            ?.blur()
    }

    companion object {
        const val REACT_CLASS = "NativeEditorFocusHost"
    }
}
