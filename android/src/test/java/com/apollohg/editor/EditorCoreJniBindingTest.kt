package com.apollohg.editor

import java.io.File
import java.net.URLClassLoader
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

internal class EditorCoreJniBindingTest {
    @Test
    fun `a loadable library without the direct entry points is not bound`() {
        val foreignLibrary = File(
            System.getProperty(JAVA_HOME_PROPERTY),
            "lib/" + System.mapLibraryName(FOREIGN_LIBRARY_NAME)
        )
        assertTrue("foreign library fixture exists at $foreignLibrary", foreignLibrary.isFile)
        val sources = listOf(EditorCoreJni::class.java, Unit::class.java).map {
            it.protectionDomain.codeSource.location
        }
        val previousOverride = System.getProperty(LIBRARY_OVERRIDE_PROPERTY)
        System.setProperty(LIBRARY_OVERRIDE_PROPERTY, foreignLibrary.absolutePath)
        try {
            URLClassLoader(sources.toTypedArray(), null).use {
                val binding = Class.forName(EditorCoreJni::class.java.name, true, it)
                val instance = binding.getField(KOTLIN_OBJECT_INSTANCE).get(null)
                val isBound = binding.getMethod(IS_BOUND_GETTER).invoke(instance) as Boolean
                assertFalse(
                    "a loaded library lacking the editor core JNI symbols must fall back",
                    isBound
                )
            }
        } finally {
            if (previousOverride == null) {
                System.clearProperty(LIBRARY_OVERRIDE_PROPERTY)
            } else {
                System.setProperty(LIBRARY_OVERRIDE_PROPERTY, previousOverride)
            }
        }
    }

    private companion object {
        const val JAVA_HOME_PROPERTY = "java.home"
        const val FOREIGN_LIBRARY_NAME = "syslookup"
        const val LIBRARY_OVERRIDE_PROPERTY = "uniffi.component.editor_core.libraryOverride"
        const val KOTLIN_OBJECT_INSTANCE = "INSTANCE"
        const val IS_BOUND_GETTER = "isBound"
    }
}
