package com.apollohg.editor.viewer

import com.apollohg.editor.NativeEditorFocusHostManager
import com.facebook.react.ReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.uimanager.ViewManager

/** Autolinked package that exposes the Fabric PreparedProseViewer and NativeEditorFocusHost managers. */
class PreparedProseViewerPackage : ReactPackage {
    // RN 0.76 requires this interface method.
    @Suppress("OVERRIDE_DEPRECATION")
    override fun createNativeModules(reactContext: ReactApplicationContext): List<NativeModule> =
        emptyList()

    override fun createViewManagers(
        reactContext: ReactApplicationContext
    ): List<ViewManager<in Nothing, in Nothing>> = listOf(
        PreparedProseViewerManager(),
        NativeEditorFocusHostManager()
    )
}
