# JNA includes desktop AWT helpers that are not available on Android. The editor
# bindings do not call those APIs, but R8 still validates the references.
-dontwarn java.awt.**

# UniFFI uses JNA reflection/proxies to bind Kotlin method and structure field
# names to the Rust library. Keep these names stable in consuming release builds.
-keep class com.sun.jna.** { *; }
-keep class uniffi.editor_core.** { *; }
-keep class com.apollohg.editor.viewer.FabricLeaseHandleBridge {
    public static void registerNativeLease(int, int, long);
    public static void finalizeNativeLease(int, int, long);
    public static void beginNativeMeasure(long);
    public static void beginNativeFinalLayout(long, int, int);
    public static void endNativeMeasure();
}
-keepattributes Signature,InnerClasses,EnclosingMethod,*Annotation*

# Preserve native editor View class names in consuming release builds so Android
# framework ViewGroup crashes identify the owning component in crash reporters.
-keepnames class com.apollohg.editor.** extends android.view.View

# JNA inspects Library interfaces and Structure fields reflectively. These
# member rules make that contract explicit for ProGuard/R8 consumers.
-keepclassmembers class * implements com.sun.jna.Library { *; }
-keepclassmembers class * extends com.sun.jna.Structure { *; }
