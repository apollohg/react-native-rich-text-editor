package com.apollohg.editor.viewer

import com.apollohg.editor.ProseViewerConfiguration
import com.apollohg.editor.ProseViewerSource
import com.apollohg.editor.tables.PlainTableFixture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@GraphicsMode(GraphicsMode.Mode.NATIVE)
internal class ProgressiveFabricGeometryTest {
    private enum class Eviction { BEFORE_YOGA, BEFORE_FINAL_LAYOUT, BEFORE_MOUNT }

    @Test
    fun initialYogaGeometryIsIndependentOfOffscreenDonorShapeResidency() {
        val registry = PreparedProseLayoutRegistry()
        val surface = FabricSurfaceToken(77, 770)
        val handle = 77L
        val donorOwner = "progressive-shape-donor"
        val sharedText = "A wrapping donor cell with several lines of exact text"
        fun request(donor: Boolean) = ProseViewerRequest(
            ProseViewerSource.Json(
                PlainTableFixture.document(130, 20) {
                        row,
                        column
                    ->
                    if (column == 19 && (row == 129 || (donor && row == 128))) {
                        sharedText
                    } else {
                        PlainTableFixture.coordinateText(row, column)
                    }
                }
            ),
            ProseViewerConfiguration(PlainTableFixture.CONFIG),
            tableGeometryPolicy =
                if (donor) TableGeometryPolicy.EAGER else TableGeometryPolicy.INITIAL,

            tableMeasurementViewportHeightPx = if (donor) 0 else 120
        )
        val donor = registry.measure(request(true), 320, 1f)
        assertNotNull(donor.blocks.first().tableSurface!!.cells.last().cachedContent?.cellShape)
        registry.registerDirectMounted(donorOwner, donor)
        registry.registerFabricLease(surface, handle)
        val request = request(false)
        val generation = FabricGenerationToken(surface, request.generationIdentity, handle)
        registry.activateFabricGeneration(generation)
        try {
            val yoga = registry.measure(request, 320, 1f, surface, handle)
            registry.releaseDirectMounted(donorOwner)
            registry.didReceiveMemoryWarning()
            val final = registry.prepareFinalLayout(request, 320, 1f, 0, 0, surface, handle)
            assertEquals(
                "Eviction must not change geometry under the initial Yoga revision",
                yoga.heightPx,
                final.heightPx
            )
            assertEquals(
                yoga.blocks.first().tableSurface!!.layout.rowOffsets,
                final.blocks.first().tableSurface!!.layout.rowOffsets
            )
            assertNotNull(yoga.blocks.first().tableSurface!!.cells.last().pendingMeasurement)
            assertSame(final, registry.acquirePreparedMountTicket(generation)!!.artifact)
        } finally {
            registry.releaseDirectMounted(donorOwner)
            registry.finalizeFabricLease(surface, handle)
        }
    }

    @Test
    fun deferredAccessibilityRevealSurvivesFabricMountAndTheFollowingPreDraw() {
        for (fontRace in listOf(false, true)) {
            val activity = org.robolectric.Robolectric.buildActivity(
                android.app.Activity::class.java
            ).setup()
            val manager = PreparedProseViewerManager()
            val context = com.facebook.react.uimanager.ThemedReactContext(
                com.facebook.react.bridge.BridgeReactContext(activity.get()),
                activity.get()
            )
            val view = PreparedProseViewerManager::class.java.getDeclaredMethod(
                "createViewInstance",
                com.facebook.react.uimanager.ThemedReactContext::class.java
            ).apply {
                isAccessible =
                    true
            }
                .invoke(manager, context) as PreparedProseDrawingView
            val scroll = android.widget.ScrollView(activity.get())
            scroll.addView(view)
            activity.get().setContentView(scroll)
            val states = PreparedProseViewerManager::class.java.getDeclaredField("states").apply {
                isAccessible =
                    true
            }
                .get(manager) as Map<*, *>
            val state = states[view] as PreparedProseViewerManager.ViewState
            state.javaClass.getDeclaredField("createStateMap").apply { isAccessible = true }
                .set(state, { com.facebook.react.bridge.JavaOnlyMap() })
            state.source = PlainTableFixture.document(130, 20, PlainTableFixture::coordinateText)
            state.configJson = PlainTableFixture.CONFIG
            val wrapper = java.lang.reflect.Proxy.newProxyInstance(
                com.facebook.react.uimanager.StateWrapper::class.java.classLoader,
                arrayOf(com.facebook.react.uimanager.StateWrapper::class.java)
            ) { _, _, _ -> null }
                as com.facebook.react.uimanager.StateWrapper
            val handle = if (fontRace) 76L else 75L
            val surface = FabricSurfaceToken(75, handle.toInt())
            state.replaceStateWrapper(
                wrapper,
                PreparedProseViewerManager.FabricStateRevisions(
                    0,
                    0,
                    handle,
                    tableGeometryPolicy = TableGeometryPolicy.INITIAL
                )
            )
            val registry = PreparedProseLayoutRegistry.shared
            registry.registerFabricLease(surface, handle)
            val install = PreparedProseViewerManager::class.java.getDeclaredMethod(
                "installPreparedTicket",
                PreparedProseDrawingView::class.java,
                PreparedProseViewerManager.ViewState::class.java,
                PreparedMountTicket::class.java
            ).apply { isAccessible = true }
            fun mount() {
                val request = state.requestOrNull()!!
                val generation = state.adopt(surface, request)
                registry.prepareFinalLayout(request, 320, 1f, 0, 0, surface, handle)
                install.invoke(
                    manager,
                    view,
                    state,
                    requireNotNull(registry.acquirePreparedMountTicket(generation))
                )
                scroll.measure(
                    android.view.View.MeasureSpec.makeMeasureSpec(
                        320,
                        android.view.View.MeasureSpec.EXACTLY
                    ),
                    android.view.View.MeasureSpec.makeMeasureSpec(
                        120,
                        android.view.View.MeasureSpec.EXACTLY
                    )
                )
                scroll.layout(0, 0, 320, 120)
                view.layout(0, 0, 320, view.preparedLayout!!.heightPx)
            }
            try {
                mount()
                assertTrue(view.isAttachedToWindow)
                val initial = view.preparedLayout!!
                val table = initial.blocks.first().tableSurface!!
                val target = table.cells.last()
                assertNotNull(target.pendingMeasurement)
                if (fontRace) state.publishFontRevision(1)
                view.revealTableAccessibilityCell(
                    view.tableAccessibilityLocation(table, target.sourceIndex)!!.cell
                )
                assertNotNull(
                    "fontRace=$fontRace: reveal must wait for the exact replacement ticket",
                    state.pendingTableReveal
                )
                mount()
                view.viewTreeObserver.dispatchOnPreDraw()
                val replacement = view.preparedLayout!!.blocks.first().tableSurface!!
                val frame = replacement.frameOfCell(target.sourceIndex)!!
                assertTrue(
                    "fontRace=$fontRace: target must remain in viewport after pre-draw; scroll=${scroll.scrollY}, target=$frame",
                    scroll.scrollY <= frame.top &&
                        scroll.scrollY + scroll.height >= frame.top + frame.height
                )
                assertNull(state.pendingTableReveal)
                if (replacement.hasPendingMeasurements) {
                    val before = view.preparedLayout!!
                    val anchor = ProgressiveTableAnchor.capture(before, scroll.scrollY)
                    val anchorScreenY = anchor.resolve(before) - scroll.scrollY
                    val settled = before.replacingTableSurfaces(
                        mapOf(
                            replacement.identity to
                                replacement.measuringRemaining { false }!!
                        )
                    )
                    PreparedProseViewerManager::class.java.getDeclaredMethod(
                        "publishTableGeometry",
                        PreparedProseDrawingView::class.java,
                        PreparedProseViewerManager.ViewState::class.java,
                        PreparedProseLayout::class.java
                    ).apply {
                        isAccessible = true
                    }.invoke(manager, view, state, settled)
                    mount()
                    view.viewTreeObserver.dispatchOnPreDraw()
                    assertEquals(
                        "Background completion must retain the visible row's screen position after native layout",
                        anchorScreenY,
                        anchor.resolve(view.preparedLayout!!) - scroll.scrollY
                    )
                }
            } finally {
                manager.onDropViewInstance(view)
                registry.finalizeFabricLease(surface, handle)
                activity.pause().stop().destroy()
            }
        }
    }

    @Test
    fun stagedStateWithoutMatchingMetadataRequestsRecoveryWithoutCompiling() {
        var compilations = 0
        val registry =
            PreparedProseLayoutRegistry(compiler = {
                compilations++
                compileWithRust(it)
            })
        val surface = FabricSurfaceToken(73, 730)
        val handle = 73L
        val request = ProseViewerRequest(
            ProseViewerSource.Json(
                PlainTableFixture.document(
                    130,
                    20,
                    PlainTableFixture::coordinateText
                )
            ),
            ProseViewerConfiguration(PlainTableFixture.CONFIG),
            tableGeometryRevision = nextTableGeometryRevision(),
            tableGeometryPolicy = TableGeometryPolicy.STAGED,
            tableMeasurementViewportHeightPx = 120
        )
        val generation = FabricGenerationToken(surface, request.generationIdentity, handle)
        registry.registerFabricLease(surface, handle)
        registry.activateFabricGeneration(generation)
        try {
            assertTrue(
                runCatching {
                    registry.measure(request, 320, 1f, surface, handle)
                }.exceptionOrNull()
                    is TableGeometryRevisionRequired
            )
            assertTrue(
                "A semantic replacement cannot wait forever for absent staged geometry",
                registry.requiresTableGeometryRevision(generation)
            )
            var completion: Boolean? = null
            assertTrue(registry.prepareForFabricMount(generation) { completion = it })
            assertEquals(false, completion)
            assertEquals(0, compilations)
        } finally {
            registry.finalizeFabricLease(surface, handle)
        }
    }

    @Test
    fun evictedProgressiveGeometryRequiresANewRevisionInsteadOfRebuildingAnOldTicket() {
        for (phase in Eviction.entries) {
            var compilations = 0
            val registry = PreparedProseLayoutRegistry(compiler = { request ->
                compilations++
                compileWithRust(request)
            }, compiledByteBudget = 0)
            val surface = FabricSurfaceToken(72, 720)
            val handle = 72L
            val initialRequest = ProseViewerRequest(
                ProseViewerSource.Json(
                    PlainTableFixture.document(130, 20, PlainTableFixture::coordinateText)
                ),
                ProseViewerConfiguration(PlainTableFixture.CONFIG),
                tableGeometryPolicy = TableGeometryPolicy.INITIAL,
                tableMeasurementViewportHeightPx = 120
            )
            fun generation(request: ProseViewerRequest) =
                FabricGenerationToken(surface, request.generationIdentity, handle)
            val initialGeneration = generation(initialRequest)
            registry.registerFabricLease(surface, handle)
            registry.activateFabricGeneration(initialGeneration)
            val initial = registry.prepareFinalLayout(
                initialRequest,
                320,
                1f,
                0,
                0,
                surface,
                handle
            )
            assertNotNull(registry.acquirePreparedMountTicket(initialGeneration))
            val table = initial.blocks.first().tableSurface!!
            assertTrue(table.hasPendingMeasurements)
            val measured = table.measuringViewport(
                table.layout.rowOffsets[90],
                table.layout.rowOffsets[91]
            )
            val partial = initial.replacingTableSurfaces(mapOf(table.identity to measured))
            val request = initialRequest.copy(
                tableGeometryRevision = partial.key.tableGeometryRevision,
                tableGeometryPolicy = TableGeometryPolicy.STAGED
            )
            assertNotNull(registry.stageTableGeometry(initialGeneration, request, partial) { true })
            val next = generation(request)
            registry.activateFabricGeneration(next)
            if (phase != Eviction.BEFORE_YOGA) {
                assertEquals(
                    partial.heightPx,
                    registry.measure(request, 320, 1f, surface, handle).heightPx
                )
            }
            if (phase == Eviction.BEFORE_MOUNT) {
                assertEquals(
                    partial.heightPx,
                    registry.prepareFinalLayout(request, 320, 1f, 0, 0, surface, handle).heightPx
                )
            }
            val preparations = registry.layoutPreparationCount
            registry.didReceiveMemoryWarning()
            if (phase != Eviction.BEFORE_MOUNT) {
                val missing = runCatching {
                    if (phase ==
                        Eviction.BEFORE_YOGA
                    ) {
                        registry.measure(request, 320, 1f, surface, handle)
                    } else {
                        registry.prepareFinalLayout(request, 320, 1f, 0, 0, surface, handle)
                    }
                }.exceptionOrNull()
                assertTrue(
                    "$phase: missing exact geometry is a remeasurement request",
                    missing is TableGeometryRevisionRequired
                )
            }
            assertNull(registry.acquirePreparedMountTicket(next))
            val completion = CountDownLatch(1)
            var prepared = true
            assertTrue(
                registry.prepareForFabricMount(next) {
                    prepared = it
                    completion.countDown()
                }
            )
            assertTrue(completion.await(5, TimeUnit.SECONDS))
            assertFalse("$phase: an old revision cannot acquire reconstructed geometry", prepared)
            assertTrue(registry.requiresTableGeometryRevision(next))
            assertEquals(
                "$phase: recovery must not silently shape under an old key",
                preparations,
                registry.layoutPreparationCount
            )
            assertEquals(
                "$phase: exact staging and misses must not recompile the source",
                1,
                compilations
            )
            val recovery = request.copy(
                tableGeometryRevision = nextTableGeometryRevision(),
                tableGeometryPolicy = TableGeometryPolicy.EAGER,
                tableMeasurementViewportHeightPx = 0
            )
            val recoveredGeneration = generation(recovery)
            registry.activateFabricGeneration(recoveredGeneration)
            val recovered = registry.prepareFinalLayout(recovery, 320, 1f, 0, 0, surface, handle)
            assertFalse(recovered.blocks.first().tableSurface!!.hasPendingMeasurements)
            assertSame(
                recovered,
                registry.acquirePreparedMountTicket(recoveredGeneration)!!.artifact
            )
            registry.finalizeFabricLease(surface, handle)
        }
    }
}
