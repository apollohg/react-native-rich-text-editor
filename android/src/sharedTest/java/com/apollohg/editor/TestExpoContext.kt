package com.apollohg.editor

import android.app.Activity
import android.content.Context
import expo.modules.core.ModuleRegistry
import expo.modules.kotlin.AppContext
import expo.modules.kotlin.ModulesProvider
import expo.modules.kotlin.modules.Module
import java.lang.ref.WeakReference

internal data class TestExpoContext(val context: Context, val appContext: AppContext)

private const val SO_LOADER_CLASS = "com.facebook.soloader.SoLoader"
private const val SO_LOADER_INIT = "init"
private const val REACT_CONTEXT_CLASS = "com.facebook.react.bridge.BridgeReactContext"
private const val HOST_RESUME = "onHostResume"
private const val APP_CONTEXT_ARITY = 3

internal fun testExpoContext(
    context: Context,
    currentActivity: Activity? = context as? Activity
): TestExpoContext {
    val reactContext = Class.forName(REACT_CONTEXT_CLASS)
        .getConstructor(Context::class.java)
        .newInstance(context) as Context
    currentActivity?.let {
        reactContext.javaClass.getMethod(HOST_RESUME, Activity::class.java).invoke(reactContext, it)
    }
    val modulesProvider = object : ModulesProvider {
        override fun getModulesMap(): Map<Class<out Module>, String?> = emptyMap()
    }
    val constructor = AppContext::class.java.constructors.first {
        it.parameterTypes.size ==
            APP_CONTEXT_ARITY
    }
    val appContext = constructor.newInstance(
        modulesProvider,
        ModuleRegistry(emptyList(), emptyList()),
        WeakReference(reactContext)
    ) as AppContext
    return TestExpoContext(reactContext, appContext)
}

internal fun initializeSoLoaderIfAvailable(context: Context) {
    val soLoader = try {
        Class.forName(SO_LOADER_CLASS)
    } catch (_: ClassNotFoundException) {
        return
    }
    soLoader.getMethod(SO_LOADER_INIT, Context::class.java, Boolean::class.javaPrimitiveType)
        .invoke(null, context, false)
}
