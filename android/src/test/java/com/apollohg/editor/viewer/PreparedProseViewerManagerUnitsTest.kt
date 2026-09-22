@file:Suppress("DEPRECATION")

package com.apollohg.editor.viewer

import android.graphics.Rect
import android.graphics.Typeface
import android.view.MotionEvent
import com.facebook.react.bridge.BridgeReactContext
import com.facebook.react.bridge.JavaOnlyMap
import com.facebook.react.uimanager.ReactStylesDiffMap
import com.facebook.react.uimanager.StateWrapper
import com.facebook.react.uimanager.ThemedReactContext
import java.lang.reflect.Proxy
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
class PreparedProseViewerManagerUnitsTest {
    @Test
    fun `Fabric viewer observes font registration before its first state mount`() {
        ViewerFontEnvironment.resetFamilyRegistryForTesting()
        val context = RuntimeEnvironment.getApplication()
        val reactContext = BridgeReactContext(context)
        val manager = PreparedProseViewerManager()
        val view = createView(manager, ThemedReactContext(reactContext, context))
        try {
            ViewerFontEnvironment.registerAvailableFamily("font-before-mount", Typeface.DEFAULT)

            assertEquals(1L, fontEnvironment(manager, view).revision)
        } finally {
            manager.onDropViewInstance(view)
            ViewerFontEnvironment.resetFamilyRegistryForTesting()
        }
    }

    @Test
    fun `font invalidation before Fabric state increments the first published revision once`() {
        ViewerFontEnvironment.resetFamilyRegistryForTesting()
        val state = PreparedProseViewerManager.ViewState(
            createStateMap = { JavaOnlyMap() }
        )
        try {
            state.fontEnvironment.onInvalidated = state::publishFontRevision
            state.fontEnvironment.activate(deliverPending = true)
            ViewerFontEnvironment.registerAvailableFamily("pending-font", Typeface.DEFAULT)
            var publishedNativeFontRevision: Double? = null
            val wrapper = Proxy.newProxyInstance(
                StateWrapper::class.java.classLoader,
                arrayOf(StateWrapper::class.java)
            ) { _, method, arguments ->
                if (method.name == "updateState") {
                    publishedNativeFontRevision =
                        (arguments?.single() as com.facebook.react.bridge.WritableMap)
                            .getDouble("nativeFontRevision")
                }
                null
            } as StateWrapper

            state.replaceStateWrapper(
                wrapper,
                PreparedProseViewerManager.FabricStateRevisions(
                    attachmentRevision = 2,
                    nativeFontRevision = 3,
                    leaseHandle = 4
                )
            )

            assertEquals(4.0, publishedNativeFontRevision)
        } finally {
            state.fontEnvironment.deactivate()
            ViewerFontEnvironment.resetFamilyRegistryForTesting()
        }
    }

    @Test
    fun `stale same-lease Fabric state cannot roll back a published font revision`() {
        val state = PreparedProseViewerManager.ViewState(
            createStateMap = { JavaOnlyMap() }
        )
        val wrapper = Proxy.newProxyInstance(
            StateWrapper::class.java.classLoader,
            arrayOf(StateWrapper::class.java)
        ) { _, _, _ -> null } as StateWrapper
        val initial = PreparedProseViewerManager.FabricStateRevisions(
            attachmentRevision = 2,
            nativeFontRevision = 3,
            leaseHandle = 4
        )
        state.replaceStateWrapper(wrapper, initial)
        state.publishFontRevision(1)

        state.replaceStateWrapper(wrapper, initial)

        assertEquals(4L, requireNotNull(state.requestOrNull()).nativeFontRevision)
    }

    @Test
    fun `pre-state reconciliation keeps observing fonts until first valid state`() {
        ViewerFontEnvironment.resetFamilyRegistryForTesting()
        val context = RuntimeEnvironment.getApplication()
        val state = PreparedProseViewerManager.ViewState(
            createStateMap = { JavaOnlyMap() }
        )
        val view = PreparedProseDrawingView(context)
        try {
            state.fontEnvironment.onInvalidated = state::publishFontRevision
            state.fontEnvironment.activate(deliverPending = true)
            state.releaseGeneration(view)

            ViewerFontEnvironment.registerAvailableFamily("after-empty-state", Typeface.DEFAULT)
            var publishedNativeFontRevision: Double? = null
            val wrapper = Proxy.newProxyInstance(
                StateWrapper::class.java.classLoader,
                arrayOf(StateWrapper::class.java)
            ) { _, method, arguments ->
                if (method.name == "updateState") {
                    publishedNativeFontRevision =
                        (arguments?.single() as com.facebook.react.bridge.WritableMap)
                            .getDouble("nativeFontRevision")
                }
                null
            } as StateWrapper
            state.replaceStateWrapper(
                wrapper,
                PreparedProseViewerManager.FabricStateRevisions(
                    attachmentRevision = 0,
                    nativeFontRevision = 0,
                    leaseHandle = 8
                )
            )

            assertEquals(1.0, publishedNativeFontRevision)
        } finally {
            state.release()
            ViewerFontEnvironment.resetFamilyRegistryForTesting()
        }
    }

    @Test
    fun `fabric constraints remain physical pixels until yoga output`() {
        val density = 2.625f
        val widthPixels = 891f

        assertEquals(891, fabricConstraintPixels(widthPixels))
        assertEquals(339.42856f, fabricPixelsToDp(widthPixels, density)!!, 0.0001f)
        assertEquals(200f, fabricPixelsToDp(525f, density)!!, 0.0001f)
        assertNull(fabricConstraintPixels(Float.POSITIVE_INFINITY))
        assertNull(fabricPixelsToDp(100f, 0f))
    }

    @Test
    fun `individual Fabric prop callbacks do not reconcile before the transaction boundary`() {
        val manager = PreparedProseViewerManager()
        val view = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        val mounted = preparedArtifact("mounted")
        view.install(mounted)

        manager.setSource(view, "staged")

        assertSame(mounted, view.preparedLayout)

        manager.updateProperties(
            view,
            ReactStylesDiffMap(JavaOnlyMap.of("source", "committed"))
        )
        assertNull(view.preparedLayout)
    }

    @Test
    fun `atom dispatch ownership rejects released and replaced Fabric leases`() {
        val state = PreparedProseViewerManager.ViewState().apply {
            revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 77)
            source = "first"
        }
        val surface = FabricSurfaceToken(71, 19)
        val registry = PreparedProseLayoutRegistry.shared
        registry.registerFabricLease(surface, 77)
        val first = state.adopt(surface, requireNotNull(state.requestOrNull()))
        val artifact = preparedArtifact("atom-owner")
        state.installAtomArtifact(first, artifact)
        assertTrue(state.ownsAtomArtifact(first, artifact))

        state.source = "replacement"
        val replacement = state.adopt(surface, requireNotNull(state.requestOrNull()))
        assertTrue(first != replacement)
        assertTrue(!state.ownsAtomArtifact(first, artifact))

        registry.releaseFabricGeneration(replacement)
        assertTrue(!state.ownsAtomArtifact(replacement, artifact))
        state.release()
    }

    @Test
    fun `final atom dispatch emits one envelope only for its active owner`() {
        val manager = PreparedProseViewerManager()
        val view = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        val state = PreparedProseViewerManager.ViewState().apply {
            source = "compiler-backed-table-owner"
            themeJson = """{"viewerAtoms":{"generation":"g1","revision":"r1","nodeTypes":["card"],"estimatedHeights":{"card":40}}}"""
            revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 88)
        }
        val surface = FabricSurfaceToken(72, 20)
        val registry = PreparedProseLayoutRegistry.shared
        registry.registerFabricLease(surface, 88)
        val generation = state.adopt(surface, requireNotNull(state.requestOrNull()))
        val artifact = preparedArtifact("atom-event").copy(
            viewerAtoms = listOf(PreparedViewerAtom("card", 17, "{\"id\":17}", Rect(3, 4, 53, 44)))
        )
        view.install(artifact)
        state.installAtomArtifact(generation, artifact)
        installState(manager, view, state)
        val events = mutableListOf<ViewerAtomLayoutEvent>()
        manager.atomLayoutEventSinkForTesting = events::add

        dispatchAtomLayout(manager, view, state)
        dispatchAtomLayout(manager, view, state)
        assertEquals(1, events.size)
        val event = events.single()
        assertEquals("g1", event.generation)
        assertEquals("r1", event.revision)
        val envelope = JSONObject(event.atomsJson)
        assertEquals("viewer-atoms-v2", envelope.getString("format"))
        assertTrue(envelope.getLong("presentationSequence") > 0)
        assertEquals(0, envelope.getJSONArray("atoms").length())

        registry.deactivateFabricLease(surface, generation.leaseHandle)
        state.installAtomArtifact(generation, artifact)
        dispatchAtomLayout(manager, view, state)
        assertEquals(1, events.size)
        state.release()
    }

    @Test
    fun `drawing view translates interactions and accessibility into the content box`() {
        val view = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        val artifact = preparedArtifact("content-origin").copy(
            interactions = listOf(
                PreparedProseInteraction(
                    kind = PreparedProseInteraction.Kind.LINK,
                    rects = listOf(Rect(0, 0, 20, 20)),
                    href = "https://example.test",
                    visibleText = "link",
                    label = "link"
                )
            ),
            accessibilityNodes = listOf(
                PreparedProseAccessibilityNode(
                    interactionIndex = 0,
                    role = PreparedProseAccessibilityNode.Role.LINK,
                    label = "link",
                    bounds = Rect(0, 0, 20, 20)
                )
            )
        )
        var activated = false
        view.onInteractionActivated = {
            activated = true
            true
        }
        view.install(artifact, contentOriginXPx = 11, contentOriginYPx = 13)

        val outside = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, 10.5f, 17f, 0)
        assertEquals(false, view.onTouchEvent(outside))
        outside.recycle()
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, 15f, 17f, 0)
        val up = MotionEvent.obtain(0, 1, MotionEvent.ACTION_UP, 15f, 17f, 0)
        assertTrue(view.onTouchEvent(down))
        assertTrue(view.onTouchEvent(up))
        down.recycle()
        up.recycle()
        assertTrue(activated)

        val bounds = Rect()
        requireNotNull(view.accessibilityNodeProvider.createAccessibilityNodeInfo(1))
            .getBoundsInParent(bounds)
        assertEquals(Rect(11, 13, 31, 33), bounds)
    }

    private fun preparedArtifact(generation: String): PreparedProseLayout = PreparedProseLayout(
        key = ProseLayoutKey(
            semanticKey = generation,
            widthPx = 100,
            themeDigest = "fixture",
            nativeFontRevision = 0,
            fontEnvironmentRevision = 0,
            densityBits = 1f.toRawBits().toLong(),
            attachmentRevision = 0,
            generationIdentity = generation
        ),
        widthPx = 100,
        heightPx = 20,
        blocks = emptyList(),
        retainedBytes = 0
    )

    @Suppress("UNCHECKED_CAST")
    private fun installState(
        manager: PreparedProseViewerManager,
        view: PreparedProseDrawingView,
        state: PreparedProseViewerManager.ViewState
    ) {
        val states = PreparedProseViewerManager::class.java.getDeclaredField("states")
            .apply { isAccessible = true }.get(manager) as MutableMap<PreparedProseDrawingView, PreparedProseViewerManager.ViewState>
        states[view] = state
    }

    private fun dispatchAtomLayout(
        manager: PreparedProseViewerManager,
        view: PreparedProseDrawingView,
        state: PreparedProseViewerManager.ViewState
    ) {
        PreparedProseViewerManager::class.java.getDeclaredMethod(
            "dispatchAtomLayout",
            PreparedProseDrawingView::class.java,
            PreparedProseViewerManager.ViewState::class.java,
            PreparedMountTicket::class.java
        ).apply { isAccessible = true }.invoke(manager, view, state, null)
    }

    @Suppress("UNCHECKED_CAST")
    private fun fontEnvironment(
        manager: PreparedProseViewerManager,
        view: PreparedProseDrawingView
    ): ViewerFontEnvironment {
        val statesField = PreparedProseViewerManager::class.java
            .getDeclaredField("states")
            .apply { isAccessible = true }
        val states = statesField.get(manager) as Map<PreparedProseDrawingView, Any>
        val state = requireNotNull(states[view])
        return state.javaClass.getDeclaredField("fontEnvironment")
            .apply { isAccessible = true }
            .get(state) as ViewerFontEnvironment
    }

    private fun createView(
        manager: PreparedProseViewerManager,
        context: ThemedReactContext
    ): PreparedProseDrawingView = PreparedProseViewerManager::class.java
        .getDeclaredMethod("createViewInstance", ThemedReactContext::class.java)
        .apply { isAccessible = true }
        .invoke(manager, context) as PreparedProseDrawingView
}
