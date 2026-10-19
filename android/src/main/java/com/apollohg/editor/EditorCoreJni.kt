package com.apollohg.editor

internal object EditorCoreJni {
    private const val LIBRARY_NAME = "editor_core"
    private const val LIBRARY_OVERRIDE_PROPERTY = "uniffi.component.editor_core.libraryOverride"

    val isBound: Boolean = try {
        val override = System.getProperty(LIBRARY_OVERRIDE_PROPERTY)
        if (override == null) System.loadLibrary(LIBRARY_NAME) else System.load(override)
        isAvailable()
    } catch (_: UnsatisfiedLinkError) {
        false
    }

    private external fun isAvailable(): Boolean

    external fun applyNativeIntent(editorId: ByteArray, requestJson: ByteArray): ByteArray?

    external fun renderNativeFrame(
        editorId: ByteArray,
        ownerId: ByteArray?,
        hasMirrorAnchor: Boolean,
        mirrorAnchor: Long,
        hasMirrorHead: Boolean,
        mirrorHead: Long
    ): ByteArray?
}
