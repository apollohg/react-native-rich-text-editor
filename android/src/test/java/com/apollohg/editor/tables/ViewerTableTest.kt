package com.apollohg.editor.tables

import com.apollohg.editor.ProseViewerConfiguration
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.ProseViewerSource
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.ProseViewerRequest
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.AndroidProseLayoutEngine
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseAccessibilityNode
import com.apollohg.editor.viewer.PreparedProseInteraction
import com.apollohg.editor.viewer.PreparedProseLayoutRegistry
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.ViewerInline
import com.apollohg.editor.viewer.ViewerDocument
import com.apollohg.editor.viewer.ViewerImageAttachment
import com.apollohg.editor.viewer.ViewerImagePipeline
import com.apollohg.editor.viewer.ViewerImageIntrinsicStore
import com.apollohg.editor.viewer.cellDocument
import com.apollohg.editor.viewer.compileWithRust
import com.apollohg.editor.viewer.PreparedProseFragmentKind
import com.apollohg.editor.viewer.ResolvedTextStyleSpan
import com.apollohg.editor.NativeCodeHighlightingConfig
import com.apollohg.editor.DecodedBitmapBudget
import com.apollohg.editor.DecodedBitmapLease
import com.apollohg.editor.DecodedBitmapPriority
import com.apollohg.editor.RenderImageLoader
import android.graphics.Typeface
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Bitmap
import android.graphics.Canvas
import android.app.Activity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityManager
import android.widget.FrameLayout
import android.widget.EditText
import android.os.Looper
import android.text.Spanned
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Robolectric
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseViewerManager
import com.apollohg.editor.viewer.FabricSurfaceToken
import com.apollohg.editor.viewer.FabricGenerationToken
import com.apollohg.editor.viewer.FabricAttachmentSidecars
import com.apollohg.editor.viewer.ViewerAtomLayoutEvent
import com.apollohg.editor.viewer.PreparedMountTicket
import com.facebook.react.bridge.BridgeReactContext
import com.facebook.react.uimanager.ThemedReactContext
import org.junit.Assert.assertFalse
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.TableRenderFailure

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ViewerTableTest {
    @Test
    fun `viewer input traversal detects and releases a temporary editor child`() {
        val root = FrameLayout(RuntimeEnvironment.getApplication())
        val editor = EditText(root.context)

        root.addView(editor)
        assertEquals(1, viewerInputSurfaceCount(root))

        root.removeView(editor)
        assertEquals(0, viewerInputSurfaceCount(root))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `deferred table image completion rejects an expired owner and publishes the current owner`() {
        data class PendingImage(
            val ownerId: Long,
            val callback: (DecodedBitmapLease?) -> Unit
        )

        val pending = mutableListOf<PendingImage>()
        val manager = PreparedProseViewerManager {
            ViewerImagePipeline(load = { _, ownerId, _, callback ->
                pending += PendingImage(ownerId, callback)
                RenderImageLoader.LoadHandle {}
            })
        }
        val registry = PreparedProseLayoutRegistry.shared
        val request = ProseViewerRequest(
            ProseViewerSource.Json(deferredImageTableSource()),
            ProseViewerConfiguration(CONFIG, imagesEnabled = true)
        )
        val firstSurface = FabricSurfaceToken(390, 3900)
        val firstGeneration = FabricGenerationToken(firstSurface, request.generationIdentity, 390L)
        val secondSurface = FabricSurfaceToken(391, 3901)
        val secondGeneration = FabricGenerationToken(secondSurface, request.generationIdentity, 391L)
        registry.registerFabricLease(firstSurface, firstGeneration.leaseHandle)
        registry.prepareFinalLayout(request, 390, 1f, 0, 0, firstSurface, firstGeneration.leaseHandle)
        registry.activateFabricGeneration(firstGeneration)
        val firstTicket = requireNotNull(registry.acquirePreparedMountTicket(firstGeneration))
        val attachment = firstTicket.artifact.imageAttachments.single()
        assertTrue(firstTicket.artifact.blocks.single().tableSurface!!.cells.isNotEmpty())

        withMountedDrawing(
            firstTicket.artifact,
            width = 390,
            height = firstTicket.artifact.heightPx.coerceAtLeast(1),
            viewFactory = { activity ->
                val context = ThemedReactContext(BridgeReactContext(activity), activity, "tables", 390)
                PreparedProseViewerManager::class.java.getDeclaredMethod(
                    "createViewInstance", ThemedReactContext::class.java
                ).apply { isAccessible = true }.invoke(manager, context) as PreparedProseDrawingView
            }
        ) { view ->
            var baseLease: DecodedBitmapLease? = null
            @Suppress("UNCHECKED_CAST")
            val states = PreparedProseViewerManager::class.java.getDeclaredField("states")
                .apply { isAccessible = true }.get(manager) as Map<PreparedProseDrawingView, PreparedProseViewerManager.ViewState>
            val state = requireNotNull(states[view]).apply {
                source = request.source.value
                configJson = CONFIG
                revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 390L)
                adopt(firstSurface, request)
                bindFabricAttachmentState(firstGeneration)
            }
            val install = PreparedProseViewerManager::class.java.getDeclaredMethod(
                "installPreparedTicket", PreparedProseDrawingView::class.java,
                PreparedProseViewerManager.ViewState::class.java, PreparedMountTicket::class.java
            ).apply { isAccessible = true }
            try {
                install.invoke(manager, view, state, firstTicket)
                view.draw(Canvas(Bitmap.createBitmap(390, firstTicket.artifact.heightPx.coerceAtLeast(1), Bitmap.Config.ARGB_8888)))
                assertEquals(1, pending.size)

                registry.deactivateFabricLease(firstSurface, firstGeneration.leaseHandle)
                registry.registerFabricLease(secondSurface, secondGeneration.leaseHandle)
                registry.prepareFinalLayout(request, 390, 1f, 0, 0, secondSurface, secondGeneration.leaseHandle)
                registry.activateFabricGeneration(secondGeneration)
                val secondTicket = requireNotNull(registry.acquirePreparedMountTicket(secondGeneration))
                assertEquals(attachment.id, secondTicket.artifact.imageAttachments.single().id)
                assertEquals(attachment.ordinal, secondTicket.artifact.imageAttachments.single().ordinal)
                state.revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 391L)
                state.adopt(secondSurface, request)
                state.bindFabricAttachmentState(secondGeneration)
                install.invoke(manager, view, state, secondTicket)
                view.draw(Canvas(Bitmap.createBitmap(390, secondTicket.artifact.heightPx.coerceAtLeast(1), Bitmap.Config.ARGB_8888)))
                assertEquals(2, pending.size)

                data class TableGeometry(
                    val imageCellHeight: Float,
                    val imageRowHeight: Float,
                    val footerCellHeight: Float,
                    val fullHeight: Int
                )
                fun geometry(artifact: PreparedProseLayout): TableGeometry {
                    val table = requireNotNull(artifact.blocks.single().tableSurface)
                    val sourceTable = requireNotNull(table.sourceTable)
                    val imageCell = table.cells.single { candidate ->
                        candidate.content.imageAttachments.any { it.id == attachment.id }
                    }
                    val imageSource = sourceTable.cells[requireNotNull(imageCell.sourceCellIndex)]
                    assertEquals(0u, imageSource.row)
                    assertEquals(attachment.id, imageCell.content.imageAttachments.single().id)
                    val footerCell = table.cells.single { candidate ->
                        sourceTable.cells[requireNotNull(candidate.sourceCellIndex)].row == 1u
                    }
                    val rowHeight = table.layout.rowOffsets[1] - table.layout.rowOffsets[0]
                    return TableGeometry(imageCell.frame.height, rowHeight, footerCell.frame.height, artifact.heightPx)
                }

                val replacementSidecar = requireNotNull(FabricAttachmentSidecars.state(secondGeneration))
                val beforeRevision = replacementSidecar.revision
                val beforeIntrinsic = replacementSidecar.intrinsicSize(attachment.ordinal)
                val beforeArtifact = requireNotNull(view.preparedLayout)
                val beforeGeometry = geometry(beforeArtifact)
                val budget = DecodedBitmapBudget(512L * 1024L)
                baseLease = requireNotNull(
                    budget.reserve(160L * 1024L, DecodedBitmapPriority.VISIBLE)?.commit(
                        Bitmap.createBitmap(100, 400, Bitmap.Config.ARGB_8888),
                        160L * 1024L
                    )
                )
                pending[0].callback(
                    requireNotNull(baseLease.fork(pending[0].ownerId, 512L * 1024L, DecodedBitmapPriority.VISIBLE))
                )
                assertEquals(beforeRevision, replacementSidecar.revision)
                assertEquals(beforeIntrinsic, replacementSidecar.intrinsicSize(attachment.ordinal))
                assertSame(beforeArtifact, view.preparedLayout)
                assertEquals(beforeGeometry, geometry(requireNotNull(view.preparedLayout)))

                pending[1].callback(
                    requireNotNull(baseLease.fork(pending[1].ownerId, 512L * 1024L, DecodedBitmapPriority.VISIBLE))
                )
                assertTrue(replacementSidecar.revision > beforeRevision)
                assertEquals(100 to 400, replacementSidecar.intrinsicSize(attachment.ordinal))
                val measuredRequest = request.copy(attachmentRevision = replacementSidecar.revision)
                val measuredGeneration = FabricGenerationToken(
                    secondSurface, measuredRequest.generationIdentity, secondGeneration.leaseHandle
                )
                registry.activateFabricGeneration(measuredGeneration)
                val afterGeometry = geometry(registry.prepareFinalLayout(
                    measuredRequest, 390, 1f, 0, 0, secondSurface, measuredGeneration.leaseHandle
                ))
                assertTrue(afterGeometry.imageCellHeight > beforeGeometry.imageCellHeight)
                assertTrue(afterGeometry.imageRowHeight > beforeGeometry.imageRowHeight)
                assertEquals(beforeGeometry.footerCellHeight, afterGeometry.footerCellHeight)
                assertTrue(afterGeometry.fullHeight > beforeGeometry.fullHeight)
            } finally {
                manager.onDropViewInstance(view)
                baseLease?.close()
                registry.finalizeFabricLease(firstSurface, firstGeneration.leaseHandle)
                registry.finalizeFabricLease(secondSurface, secondGeneration.leaseHandle)
            }
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `mounted table offsets contribute to surface sidecar bytes and reset on replacement`() {
        val layout = prepare(
            """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"left"}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]}]}]}"""
        )
        val surface = requireNotNull(layout.blocks.single().tableSurface)
        val manager = PreparedProseViewerManager()

        withMountedDrawing(
            layout,
            width = 120,
            height = 80,
            viewFactory = { activity ->
                val context = ThemedReactContext(BridgeReactContext(activity), activity, "tables", 390)
                PreparedProseViewerManager::class.java.getDeclaredMethod(
                    "createViewInstance",
                    ThemedReactContext::class.java
                ).apply { isAccessible = true }.invoke(manager, context) as PreparedProseDrawingView
            }
        ) { view ->
            val before = manager.retainedSurfaceBytesForTesting(view)

            view.setTableLogicalOffset(surface.identity, 600f)
            val scrolled = manager.retainedSurfaceBytesForTesting(view)
            assertTrue(scrolled > before)

            view.install(layout.copy())
            assertEquals(before, manager.retainedSurfaceBytesForTesting(view))
            manager.onDropViewInstance(view)
            assertEquals(0L, manager.retainedSurfaceBytesForTesting(view))
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `real table registry reuses parent and nested cell artifacts at one physical width`() {
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        val request = ProseViewerRequest(
            ProseViewerSource.Json(nestedHeaderImageSource()),
            ProseViewerConfiguration(CONFIG, imagesEnabled = true)
        )
        val surface = FabricSurfaceToken(390, 3900)
        val generation = FabricGenerationToken(surface, request.generationIdentity, 390L)
        registry.registerFabricLease(surface, generation.leaseHandle)

        val prepared = registry.prepareFinalLayout(request, 390, 1f, 0, 0, surface, 390L)
        registry.activateFabricGeneration(generation)
        val ticket = requireNotNull(registry.acquirePreparedMountTicket(generation))
        val parent = ticket.artifact
        val table = requireNotNull(parent.blocks.single { it.tableSurface != null }.tableSurface)
        val cells = table.cells.map { it.content }
        val nested = cells.first().blocks.single { it.tableSurface != null }.tableSurface!!
        val nestedCells = nested.cells.map { it.content }
        val initialPreparations = preparations.size

        assertSame(prepared, parent)
        assertTrue(initialPreparations > 0)

        repeat(1_000) {
            val repeatPrepared = registry.prepareFinalLayout(request, 390, 1f, 0, 0, surface, 390L)
            val repeatTicket = requireNotNull(registry.acquirePreparedMountTicket(generation))
            assertSame(parent, repeatPrepared)
            assertSame(parent, repeatTicket.artifact)
            val repeatedTable = repeatTicket.artifact.blocks.single { it.tableSurface != null }.tableSurface!!
            assertSame(table, repeatedTable)
            assertEquals(cells.size, repeatedTable.cells.size)
            repeatedTable.cells.zip(cells).forEach { (cell, content) -> assertSame(content, cell.content) }
            val repeatedNested = repeatedTable.cells.first().content.blocks.single {
                it.tableSurface != null
            }.tableSurface!!
            assertSame(nested, repeatedNested)
            assertEquals(nestedCells.size, repeatedNested.cells.size)
            repeatedNested.cells.zip(nestedCells).forEach { (cell, content) -> assertSame(content, cell.content) }
        }

        assertEquals(initialPreparations, preparations.size)
        registry.releaseFabricGeneration(generation)
        registry.finalizeFabricLease(surface, generation.leaseHandle)
    }

    @Test
    fun `unrelated prose revision keeps rich table cells prepared while refreshing anchors`() {
        val theme = """{"viewerAtoms":{"generation":"cell-reuse","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":36}}}"""
        val configuration = ProseViewerConfiguration(interactionConfig(), themeJson = theme, imagesEnabled = true)
        val initialRequest = ProseViewerRequest(
            ProseViewerSource.Json(cellReuseSource("before table")),
            configuration
        )
        val replacementRequest = ProseViewerRequest(
            ProseViewerSource.Json(cellReuseSource("updated pre-table prose ".repeat(80))),
            configuration
        )
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        val surface = FabricSurfaceToken(777, 7770)
        val leaseHandle = 777L
        val initialGeneration = FabricGenerationToken(surface, initialRequest.generationIdentity, leaseHandle)
        val replacementGeneration = FabricGenerationToken(surface, replacementRequest.generationIdentity, leaseHandle)
        registry.registerFabricLease(surface, leaseHandle)

        try {
            registry.prepareFinalLayout(initialRequest, 390, 1f, 0, 0, surface, leaseHandle)
            registry.activateFabricGeneration(initialGeneration)
            val initial = requireNotNull(registry.acquirePreparedMountTicket(initialGeneration)).artifact
            val initialPreparationCount = preparations.size
            assertTrue(initialPreparationCount > 0)
            val initialSnapshot = ViewerTablePresentation.project(
                initial,
                ViewerTablePresentationOwner(),
                ViewerTablePresentationViewport.Unknown
            )
            val initialAtom = initialSnapshot.atoms.single { it.atom.nodeType == "card" }
            val initialLink = initialSnapshot.interactions.single {
                it.interaction.href == "https://cell.example/link"
            }
            val initialImage = initial.imageAttachments.single()

            val authoredReplacement = compileWithRust(replacementRequest)
            val authoredTable = requireNotNull(authoredReplacement.blocks.single { it.table != null }.table)
            val authoredAtoms = authoredTable.cells.flatMap { cell ->
                authoredReplacement.cellDocument(cell).blocks.flatMap { block ->
                    block.inlines.filterIsInstance<ViewerInline.Atom>()
                }
            }
            val authoredCard = authoredAtoms.single { it.nodeType == "card" }
            val authoredImage = authoredAtoms.single { it.nodeType == "image" }
            val expectedImageId = "${authoredImage.docPos}:https://example.test/reuse.png"
            val expectedSourcePositions = mutableListOf<Int>()
            fun appendSourcePositions(table: uniffi.editor_core.FfiViewerTable) {
                table.cells.forEach { cell ->
                    expectedSourcePositions += cell.sourcePos.toInt()
                    cell.elements.filterIsInstance<FfiViewerElement.Table>().forEach { nested ->
                        appendSourcePositions(requireNotNull(authoredReplacement.tableRecords[nested.tableId]))
                    }
                }
            }
            appendSourcePositions(authoredTable)
            assertTrue(authoredReplacement.semanticKey != initial.key.semanticKey)
            assertTrue(authoredCard.docPos != initialAtom.atom.docPos)
            assertTrue(expectedImageId != initialImage.id)
            assertTrue(expectedSourcePositions != initialSnapshot.cells.map { it.sourcePosition })

            registry.activateFabricGeneration(replacementGeneration)
            registry.prepareFinalLayout(replacementRequest, 390, 1f, 0, 0, surface, leaseHandle)
            val replacement = requireNotNull(
                registry.acquirePreparedMountTicket(replacementGeneration)
            ).artifact
            val replacementSnapshot = ViewerTablePresentation.project(
                replacement,
                ViewerTablePresentationOwner(),
                ViewerTablePresentationViewport.Unknown
            )
            val replacementAtom = replacementSnapshot.atoms.single { it.atom.nodeType == "card" }
            val replacementLink = replacementSnapshot.interactions.single {
                it.interaction.href == "https://cell.example/link"
            }
            val replacementImage = replacement.imageAttachments.single()

            assertEquals(authoredCard.docPos, replacementAtom.atom.docPos)
            assertEquals(expectedImageId, replacementImage.id)
            assertEquals("https://cell.example/link", replacementLink.interaction.href)
            assertTrue(replacementLink.sourceIdentity.startsWith("${authoredReplacement.semanticKey}:"))
            assertTrue(replacementLink.sourceIdentity != initialLink.sourceIdentity)
            assertEquals(expectedSourcePositions, replacementSnapshot.cells.map { it.sourcePosition })
            assertEquals(
                initialSnapshot.accessibilityNodes.map { it.node.role to it.node.label },
                replacementSnapshot.accessibilityNodes.map { it.node.role to it.node.label }
            )
            assertTrue(
                replacementSnapshot.accessibilityNodes.all {
                    it.sourceIdentity.startsWith("${authoredReplacement.semanticKey}:")
                }
            )

            assertEquals(
                "unrelated prose must not prepare unchanged table cells",
                initialPreparationCount,
                preparations.size
            )
        } finally {
            registry.releaseFabricGeneration(replacementGeneration)
            registry.finalizeFabricLease(surface, leaseHandle)
        }
    }

    @Test
    fun `changed rich cell rebuilds its shape while unchanged nested and sibling cells reuse`() {
        val configuration = ProseViewerConfiguration(
            interactionConfig(),
            themeJson = """{"viewerAtoms":{"generation":"cell-reuse","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":36}}}""",
            imagesEnabled = true
        )
        val initialRequest = ProseViewerRequest(
            ProseViewerSource.Json(cellReuseSource("before table", "linked cell")), configuration
        )
        val replacementRequest = ProseViewerRequest(
            ProseViewerSource.Json(cellReuseSource("before table", "linked cells")), configuration
        )
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)

        val initial = registry.measure(initialRequest, 390, 1f)
        val initialTable = requireNotNull(initial.blocks.single { it.tableSurface != null }.tableSurface)
        val initialSibling = initialTable.cells.last().frame
        val initialChangedShape = initialTable.cells.first().content.cellShape
        val initialNestedShape = initialTable.cells.first().content.blocks.single {
            it.tableSurface != null
        }.tableSurface!!.cells.single().content.cellShape
        val initialSiblingShape = initialTable.cells.last().content.cellShape
        val initialPreparations = preparations.size

        val replacement = registry.measure(replacementRequest, 390, 1f)
        val replacementTable = requireNotNull(replacement.blocks.single { it.tableSurface != null }.tableSurface)

        assertEquals(3, initialPreparations)
        assertEquals(initialPreparations + 1, preparations.size)
        assertEquals(replacementTable.cells.first().sourcePosition, preparations.last())
        assertEquals("linked cells", replacementTable.cells.first().content.interactions.single().visibleText)
        assertNotSame(initialChangedShape, replacementTable.cells.first().content.cellShape)
        assertSame(
            initialNestedShape,
            replacementTable.cells.first().content.blocks.single { it.tableSurface != null }
                .tableSurface!!.cells.single().content.cellShape
        )
        assertSame(initialSiblingShape, replacementTable.cells.last().content.cellShape)
        assertEquals(initialSibling, replacementTable.cells.last().frame)
        assertEquals(initialTable.layout.rowOffsets, replacementTable.layout.rowOffsets)
    }

    @Test
    fun `nested intrinsic image rebuilds its leaf and ancestor while sibling reuses`() {
        val source = nestedHeaderImageSource(
            imageSource = "https://example.test/cell-reuse-intrinsic.png",
            declaredImageSize = false,
            laterOuterRow = true
        )
        val request = ProseViewerRequest(
            ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, imagesEnabled = true)
        )
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        val initial = registry.measure(request, 390, 1f)
        val initialTable = requireNotNull(initial.blocks.single { it.tableSurface != null }.tableSurface)
        val initialAncestor = initialTable.cells.first().content.cellShape
        val initialNested = initialTable.cells.first().content.blocks.single { it.tableSurface != null }
            .tableSurface!!.cells.first().content.cellShape
        val initialSibling = initialTable.cells.last().content.cellShape
        val initialLater = initialTable.cells.first { it.frame.top > 0f }
        val initialCount = preparations.size
        val image = initial.imageAttachments.single()

        ViewerImageIntrinsicStore.shared.store(image.id, 100 to 400)
        val replacement = registry.measure(request.copy(attachmentRevision = 1), 390, 1f)
        val replacementTable = requireNotNull(replacement.blocks.single { it.tableSurface != null }.tableSurface)

        assertEquals(initialCount + 2, preparations.size)
        assertNotSame(initialAncestor, replacementTable.cells.first().content.cellShape)
        assertNotSame(
            initialNested,
            replacementTable.cells.first().content.blocks.single { it.tableSurface != null }
                .tableSurface!!.cells.first().content.cellShape
        )
        assertSame(initialSibling, replacementTable.cells.last().content.cellShape)
        assertTrue(replacementTable.layout.contentHeight > initialTable.layout.contentHeight)
        val replacementLater = replacementTable.cells.first { it.frame.top > 0f }
        assertSame(initialLater.content.cellShape, replacementLater.content.cellShape)
        assertTrue(replacementLater.frame.top > initialLater.frame.top)
        assertEquals(initialLater.frame.height, replacementLater.frame.height, 0f)
    }

    @Test
    fun `shifted prose reuses loaded undeclared image shapes with current attachment ids`() {
        val initialRequest = ProseViewerRequest(
            ProseViewerSource.Json(nestedHeaderImageSource(
                imageSource = "https://example.test/shifted-intrinsic.png",
                declaredImageSize = false,
                beforeText = "before"
            )),
            ProseViewerConfiguration(CONFIG, imagesEnabled = true)
        )
        val replacementRequest = initialRequest.copy(source = ProseViewerSource.Json(
            nestedHeaderImageSource(
                imageSource = "https://example.test/shifted-intrinsic.png",
                declaredImageSize = false,
                beforeText = "shifted prose ".repeat(80)
            )
        ))
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        val initial = registry.measure(initialRequest, 390, 1f)
        val initialImage = initial.imageAttachments.single()
        ViewerImageIntrinsicStore.shared.store(initialImage.id, 100 to 400)
        val loaded = registry.measure(initialRequest.copy(attachmentRevision = 1), 390, 1f)
        val loadedTable = requireNotNull(loaded.blocks.single { it.tableSurface != null }.tableSurface)
        val loadedShapes = loadedTable.cells.map { it.content.cellShape }

        val replacement = registry.measure(replacementRequest.copy(attachmentRevision = 1), 390, 1f)
        val replacementTable = requireNotNull(replacement.blocks.single { it.tableSurface != null }.tableSurface)
        val replacementImage = replacement.imageAttachments.single()
        val replacementDocument = compileWithRust(replacementRequest)
        val freshKey = ProseLayoutKey(
            replacementDocument.semanticKey,
            390,
            replacementRequest.themeDigest,
            0,
            0,
            1f.toRawBits().toLong(),
            1,
            replacementRequest.generationIdentity,
            replacementRequest.semanticGenerationIdentity
        )
        val fresh = StaticLayoutAndroidProseLayoutEngine().prepare(
            replacementDocument,
            freshKey,
            PreparedProseTheme.resolve(null, 1f),
            390,
            1f,
            false,
            replacementRequest.semanticGenerationIdentity
        )

        assertEquals(5, preparations.size)
        assertTrue(initialImage.id != replacementImage.id)
        assertEquals(initialImage.source, replacementImage.source)
        loadedShapes.zip(replacementTable.cells.map { it.content.cellShape }).forEach { (old, new) ->
            assertSame(old, new)
        }
        assertEquals(fresh.imageAttachments.single().bounds.height(), replacementImage.bounds.height())
    }

    @Test
    fun `identical cells share a shape while retaining distinct current bindings`() {
        val request = ProseViewerRequest(
            ProseViewerSource.Json(identicalLinkCellsSource()),
            ProseViewerConfiguration(interactionConfig(), imagesEnabled = true)
        )
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val layout = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
            .measure(request, 390, 1f)
        val cells = requireNotNull(layout.blocks.single { it.tableSurface != null }.tableSurface).cells
        val first = cells[0].content
        val second = cells[1].content

        assertEquals(1, preparations.size)
        assertSame(first.cellShape, second.cellShape)
        assertNotSame(first, second)
        assertEquals("same link", first.interactions.single().visibleText)
        assertEquals("same link", second.interactions.single().visibleText)
        assertTrue(cells[0].sourcePosition != cells[1].sourcePosition)
        val links = ViewerTablePresentation.project(
            layout,
            ViewerTablePresentationOwner(),
            ViewerTablePresentationViewport.Unknown
        ).interactions.filter { it.interaction.href == "https://same.example/link" }
        assertEquals(2, links.size)
        assertTrue(links[0].sourceIdentity != links[1].sourceIdentity)
    }

    @Test
    fun `cell shape tracks width resolved style and revision backed font scale`() {
        val configuration = ProseViewerConfiguration(interactionConfig(), imagesEnabled = true)
        val request = ProseViewerRequest(ProseViewerSource.Json(identicalLinkCellsSource()), configuration)
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        fun first(layout: PreparedProseLayout) = requireNotNull(
            layout.blocks.single { it.tableSurface != null }.tableSurface
        ).cells.first().content

        val initial = first(registry.measure(request, 390, 1f))
        val wider = first(registry.measure(request, 540, 1f))
        val styled = first(
            registry.measure(
                request.copy(configuration = configuration.copy(
                    themeJson = """{"version":1,"styles":{"text":{"fontSize":30,"lineHeight":48}}}"""
                )),
                390,
                1f
            )
        )
        val nativeRevision = first(registry.measure(request.copy(nativeFontRevision = 1), 390, 1f))
        val environmentRevision = first(registry.measure(request.copy(fontEnvironmentRevision = 1), 390, 1f))
        val scaled = first(registry.measure(request.copy(nativeFontRevision = 2), 390, 1f, fontScale = 1.5f))

        assertNotSame(initial.cellShape, wider.cellShape)
        assertNotSame(initial.cellShape, styled.cellShape)
        assertNotSame(initial.cellShape, nativeRevision.cellShape)
        assertNotSame(initial.cellShape, environmentRevision.cellShape)
        assertNotSame(initial.cellShape, scaled.cellShape)
        assertTrue(wider.widthPx > initial.widthPx)
        assertTrue(styled.heightPx > initial.heightPx)
        assertEquals("same link", scaled.interactions.single().visibleText)
        assertEquals(6, preparations.size)
    }

    @Test
    fun `viewer atom revision alone reuses while its local measurement reshapes only its cell`() {
        val source = cellReuseSource("before table")
        val probe = ProseViewerRequest(
            ProseViewerSource.Json(source),
            ProseViewerConfiguration(interactionConfig(), imagesEnabled = true)
        )
        val authored = compileWithRust(probe)
        val authoredTable = requireNotNull(authored.blocks.single { it.table != null }.table)
        val atomPosition = authored.cellDocument(authoredTable.cells.first()).blocks.single {
            (it.inlines.singleOrNull() as? ViewerInline.Atom)?.nodeType == "card"
        }.inlines.single() as ViewerInline.Atom
        val probeTheme = viewerAtomTheme("probe", atomPosition.docPos, 36, 0)
        val probeLayout = PreparedProseLayoutRegistry(compiler = ::compileWithRust).measure(
            probe.copy(configuration = probe.configuration.copy(themeJson = probeTheme)),
            390,
            1f
        )
        val atomWidth = requireNotNull(
            probeLayout.blocks.single { it.tableSurface != null }.tableSurface
        ).cells.first().content.viewerAtoms.single().bounds.width()
        val configuration = ProseViewerConfiguration(
            interactionConfig(),
            themeJson = viewerAtomTheme("1", atomPosition.docPos, 36, atomWidth),
            imagesEnabled = true
        )
        val initialRequest = probe.copy(configuration = configuration)
        val preparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = preparations::add
        }
        val registry = PreparedProseLayoutRegistry(compiler = ::compileWithRust, layoutEngine = engine)
        fun shapes(layout: PreparedProseLayout): Pair<Any?, Any?> {
            val table = requireNotNull(layout.blocks.single { it.tableSurface != null }.tableSurface)
            return table.cells.first().content.cellShape to table.cells.last().content.cellShape
        }

        val initial = registry.measure(initialRequest, 390, 1f)
        val initialShapes = shapes(initial)
        val bookkeepingOnly = registry.measure(
            initialRequest.copy(configuration = configuration.copy(
                themeJson = viewerAtomTheme("2", atomPosition.docPos, 36, atomWidth)
            )),
            390,
            1f
        )
        val bookkeepingShapes = shapes(bookkeepingOnly)
        val resized = registry.measure(
            initialRequest.copy(configuration = configuration.copy(
                themeJson = viewerAtomTheme("3", atomPosition.docPos, 180, atomWidth)
            )),
            390,
            1f
        )
        val resizedTable = requireNotNull(resized.blocks.single { it.tableSurface != null }.tableSurface)

        assertSame(initialShapes.first, bookkeepingShapes.first)
        assertSame(initialShapes.second, bookkeepingShapes.second)
        assertNotSame(initialShapes.first, resizedTable.cells.first().content.cellShape)
        assertSame(initialShapes.second, resizedTable.cells.last().content.cellShape)
        assertTrue(resizedTable.cells.first().content.heightPx > initial.blocks.single {
            it.tableSurface != null
        }.tableSurface!!.cells.first().content.heightPx)
        assertEquals(4, preparations.size)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `released rich table artifacts evict under the thirty two MiB unmounted budget`() {
        val ceiling = 32L * 1024L * 1024L
        var cellPreparations = 0
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = { cellPreparations += 1 }
        }
        val registry = PreparedProseLayoutRegistry(
            compiler = ::compileWithRust,
            layoutEngine = engine,
            byteBudget = ceiling
        )
        val request = ProseViewerRequest(
            ProseViewerSource.Json(tableCachePressureSource()),
            ProseViewerConfiguration(CONFIG, imagesEnabled = true)
        )
        var attemptedBytes = 0L
        var revision = 0L

        while (attemptedBytes <= ceiling && revision < 64L) {
            val current = request.copy(nativeFontRevision = revision)
            val surface = FabricSurfaceToken(500 + revision.toInt(), 5000 + revision.toInt())
            val generation = FabricGenerationToken(surface, current.generationIdentity, revision + 1L)
            registry.registerFabricLease(surface, generation.leaseHandle)
            val prepared = registry.prepareFinalLayout(
                current, 390, 1f, 0, 0, surface, generation.leaseHandle
            )
            registry.activateFabricGeneration(generation)
            val ticket = requireNotNull(registry.acquirePreparedMountTicket(generation))

            assertSame(prepared, ticket.artifact)
            assertTrue(ticket.artifact.blocks.any { it.tableSurface != null })
            assertTrue(ticket.artifact.blocks.flatMap { it.tableSurface?.cells.orEmpty() }.isNotEmpty())
            attemptedBytes += ticket.artifact.retainedBytes
            registry.releaseFabricGeneration(generation)
            registry.finalizeFabricLease(surface, generation.leaseHandle)
            revision += 1L
        }

        assertTrue("fixture must apply real table pressure", attemptedBytes > ceiling)
        assertTrue(registry.layoutRetainedBytesForTesting <= ceiling)
        assertEquals(0, registry.fabricLeaseCountForTesting)
        val preparationsBeforeReacquiringFirst = registry.layoutPreparationCount
        val cellPreparationsBeforeReacquiringFirst = cellPreparations
        val firstSurface = FabricSurfaceToken(900, 9000)
        val firstGeneration = FabricGenerationToken(firstSurface, request.generationIdentity, 900L)
        registry.registerFabricLease(firstSurface, firstGeneration.leaseHandle)
        registry.prepareFinalLayout(request, 390, 1f, 0, 0, firstSurface, firstGeneration.leaseHandle)

        assertTrue(registry.layoutPreparationCount > preparationsBeforeReacquiringFirst)
        assertTrue(cellPreparations > cellPreparationsBeforeReacquiringFirst)
        registry.releaseFabricGeneration(firstGeneration)
        registry.finalizeFabricLease(firstSurface, firstGeneration.leaseHandle)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `mounted viewport bounds table candidates while detached draw retains metadata`() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_header","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"five"}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"six"}]}]}]}]}]}"""
        val cellPreparations = mutableListOf<Int>()
        val engine = StaticLayoutAndroidProseLayoutEngine().apply {
            tableCellPreparationObserver = { cellPreparations += it }
        }
        val document = compileWithRust(
            ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, imagesEnabled = true))
        )
        val layout = prepare(document, engine = engine)
        val surface = requireNotNull(layout.blocks.single().tableSurface)
        assertTrue(surface.bounds.width() > surface.hostViewportWidth)
        val childLayouts = surface.cells.map { it.content }
        assertEquals(surface.cells.size, childLayouts.map { it.key.semanticKey }.toSet().size)
        assertEquals(surface.cells.map { it.sourcePosition }.toSet(), cellPreparations.toSet())
        val preparedInitially = cellPreparations.size
        assertTrue(preparedInitially > 0)

        withMountedDrawing(layout, width = 120, height = 45) { drawing ->
            val mounted = mutableListOf<Int>()
            val chrome = mutableListOf<Int>()
            var rich = 0
            drawing.onMountedTableCellsDrawnForTesting = { mounted += it }
            drawing.onTableChromeDrawnForTesting = { chrome += it }
            drawing.onTableRichFragmentDrawnForTesting = { rich += 1 }
            val initialExpected = surface.layout.sourceOrder.filterIndexed { index, _ -> index % 2 == 0 }
            fun drawTiny() {
                val canvas = Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888))
                canvas.clipRect(0, 0, 2, 2)
                drawing.draw(canvas)
            }
            drawTiny()
            assertEquals(initialExpected.size, mounted.last())
            assertEquals(initialExpected, chrome)
            assertEquals(preparedInitially, cellPreparations.size)
            surface.cells.zip(childLayouts).forEach { (cell, original) -> assertSame(original, cell.content) }
            chrome.clear()
            rich = 0
            drawing.draw(Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888)))
            assertEquals(initialExpected, chrome)
            assertTrue(rich > 0)
            assertTrue(rich < surface.cells.size)
            drawTiny()
            assertEquals(preparedInitially, cellPreparations.size)
            chrome.clear()
            drawing.setTableLogicalOffset(surface.identity, 800f)
            val offsetExpected = surface.layout.sourceOrder.filterIndexed { index, _ -> index % 2 == 1 }
            drawTiny()
            assertEquals(offsetExpected, chrome)
            assertTrue(offsetExpected != initialExpected)
            assertEquals(preparedInitially, cellPreparations.size)
            drawing.setTableLogicalOffset(surface.identity, 0f)
            chrome.clear()
            rich = 0
            drawing.draw(Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888)))
            assertTrue(rich > 0)
            drawing.translationY = 10_000f
            chrome.clear()
            rich = 0
            drawing.draw(Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888)))
            assertEquals(0, mounted.last())
            assertTrue(chrome.isEmpty())
            assertEquals(0, rich)
            surface.cells.zip(childLayouts).forEach { (cell, original) -> assertSame(original, cell.content) }
            drawing.translationY = 0f
            drawing.visibility = android.view.View.INVISIBLE
            chrome.clear()
            rich = 0
            drawing.draw(Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888)))
            assertEquals(0, mounted.last())
            assertTrue(chrome.isEmpty())
            assertEquals(0, rich)
        }

        val detached = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        val detachedMounted = mutableListOf<Int>()
        val detachedChrome = mutableListOf<Int>()
        detached.install(layout, contentOriginXPx = 7, contentOriginYPx = 11)
        detached.onMountedTableCellsDrawnForTesting = { detachedMounted += it }
        detached.onTableChromeDrawnForTesting = { detachedChrome += it }
        detached.layout(0, 0, 120, 45)
        detached.draw(Canvas(Bitmap.createBitmap(120, 45, Bitmap.Config.ARGB_8888)))
        assertEquals(surface.cells.size, detachedMounted.single())
        assertEquals(surface.layout.sourceOrder, detachedChrome)
        assertEquals(preparedInitially, cellPreparations.size)
        surface.cells.zip(childLayouts).forEach { (cell, original) -> assertSame(original, cell.content) }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `compiler admits no overlimit grid and typed fallback paints with adjacent prose`() {
        val source = gridLimitSource()
        val admissionError = runCatching {
            compileWithRust(
                ProseViewerRequest(
                    ProseViewerSource.Json(source),
                    ProseViewerConfiguration(configWithGridSlots(1), imagesEnabled = true)
                )
            )
        }.exceptionOrNull() as? ProseViewerError
        assertEquals("DOCUMENT_LIMIT_EXCEEDED", requireNotNull(admissionError).code.value)

        val compiled = compileWithRust(
            ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, imagesEnabled = true))
        )
        val sourceTable = requireNotNull(compiled.blocks.single { it.table != null }.table)
        assertTrue(sourceTable.cells.isNotEmpty())
        assertTrue(sourceTable.sourceRows.isNotEmpty())
        val failedTable = sourceTable.copy(
            rows = 0u,
            columns = 0u,
            columnWidths = emptyList(),
            irregular = true,
            sourceRows = emptyList(),
            cells = emptyList(),
            syntheticRegions = emptyList(),
            failure = TableRenderFailure.GRID_LIMIT,
            compatibilityDiagnostic = null
        )
        val failureDocument = compiled.copy(
            blocks = compiled.blocks.map { block ->
                if (block.table?.tablePos == sourceTable.tablePos) block.copy(table = failedTable) else block
            },
            tableRecords = compiled.tableRecords + ("t${sourceTable.tablePos}" to failedTable)
        )
        val layout = prepare(failureDocument)
        val table = layout.blocks.single { it.tableSurface != null }
        val surface = requireNotNull(table.tableSurface)
        assertEquals(TableLayoutFailure.GRID_LIMIT, surface.layout.failure)
        assertEquals(TableRenderFailure.GRID_LIMIT, surface.layout.typedFailure)
        assertNull(surface.preparationError)
        assertTrue(surface.cells.isEmpty())
        assertTrue(surface.layout.sourceOrder.isEmpty())
        assertTrue(layout.interactions.isEmpty())
        val frame = requireNotNull(table.tableBounds)
        assertTrue(frame.width() > 0 && frame.height() > 0)
        assertTrue(frame.left < frame.right && frame.top < frame.bottom)

        withMountedDrawing(layout) { drawing ->
            val rendered = Bitmap.createBitmap(327, layout.heightPx + 11, Bitmap.Config.ARGB_8888)
            drawing.draw(Canvas(rendered))
            val paintedFrame = Rect(frame).apply { offset(7, 11) }
            assertTrue(paintedFrame.left >= 0 && paintedFrame.top >= 0)
            assertTrue(paintedFrame.right <= rendered.width && paintedFrame.bottom <= rendered.height)
            assertTrue(
                "typed fallback must draw its red failure frame",
                (paintedFrame.top until paintedFrame.bottom).any { y ->
                    (paintedFrame.left until paintedFrame.right).any { x ->
                        rendered.getPixel(x, y).let { pixel ->
                            pixel ushr 24 > 0 &&
                                (pixel ushr 16 and 0xff) > 200 &&
                                (pixel ushr 8 and 0xff) < 100 &&
                                (pixel and 0xff) < 100
                        }
                    }
                }
            )
            val prose = layout.blocks.filter { it.tableSurface == null }
            assertEquals(2, prose.size)
            assertTrue(
                "compiler-prepared prose before and after the fallback must retain ink",
                prose.all { block ->
                    val bounds = requireNotNull(block.fragments.firstOrNull { it.kind == PreparedProseFragmentKind.TEXT }?.bounds)
                    (bounds.top + 11 until bounds.bottom + 11).any { y ->
                        (bounds.left + 7 until bounds.right + 7).any { x ->
                            rendered.getPixel(x, y).let { pixel ->
                                pixel ushr 24 > 0 &&
                                    (pixel ushr 16 and 0xff) < 200 &&
                                    (pixel ushr 8 and 0xff) < 200 &&
                                    (pixel and 0xff) < 200
                            }
                        }
                    }
                }
            )
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `compiler backed first cell link clips touch and provider bounds with nonzero origin`() {
        val config = CONFIG.replace("\"marks\":[{\"name\":\"bold\"}]", "\"marks\":[{\"name\":\"bold\"},{\"name\":\"link\",\"attrs\":{\"href\":{}}}]")
        val layout = prepare(
            """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before","marks":[{"type":"link","attrs":{"href":"https://before.example"}}]}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"link","attrs":{"href":"https://cell-one.example"}}]}]}]},{"type":"table_cell","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"link","attrs":{"href":"https://cell-two.example"}}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after","marks":[{"type":"link","attrs":{"href":"https://after.example"}}]}]}]}""",
            config = config
        )
        withMountedDrawing(layout) { view ->
            val activated = mutableListOf<String>()
            view.onInteractionActivated = { activated += it.href.orEmpty(); true }
            val initial = ViewerTablePresentation.project(layout, ViewerTablePresentationOwner(), ViewerTablePresentationViewport.Unknown)
            fun hrefOf(node: ViewerTablePresentedAccessibilityNode): String? =
                node.node.interactionIndex?.let { node.layout.interactions.getOrNull(it)?.href }
            fun idFor(snapshot: ViewerTablePresentationSnapshot, href: String): Int =
                snapshot.accessibilityNodes.indexOfFirst { hrefOf(it) == href } + 1
            val first = initial.accessibilityNodes.first { hrefOf(it) == "https://cell-one.example" }
            val surface = layout.blocks.first { it.tableSurface != null }.tableSurface!!
            val tableBounds = layout.blocks.first { it.tableSurface?.identity == surface.identity }.tableBounds!!
            val partialOffset = first.bounds.centerX() - tableBounds.left
            assertTrue(partialOffset > 0f)
            val partialOwner = ViewerTablePresentationOwner().apply { setLogicalOffset(partialOffset, surface) }
            val partialSnapshot = ViewerTablePresentation.project(layout, partialOwner, ViewerTablePresentationViewport.Unknown)
            val partial = partialSnapshot.accessibilityNodes.first { hrefOf(it) == "https://cell-one.example" }
            val rawPartialBounds = RectF(partial.bounds)
            val partialBounds = RectF(rawPartialBounds)
            assertTrue(partialBounds.intersect(partial.clip))
            assertTrue(partialBounds.width() > 0f)
            assertTrue(partialBounds.width() < first.bounds.width())

            view.setTableLogicalOffset(surface.identity, partialOffset)
            val provider = view.accessibilityNodeProvider
            val partialId = idFor(partialSnapshot, "https://cell-one.example")
            val partialInfo = requireNotNull(provider.createAccessibilityNodeInfo(partialId))
            val parent = Rect(); partialInfo.getBoundsInParent(parent)
            val screen = Rect(); partialInfo.getBoundsInScreen(screen)
            val expectedParent = Rect(
                partialBounds.left.toInt() + 7,
                partialBounds.top.toInt() + 11,
                partialBounds.right.toInt() + 7,
                partialBounds.bottom.toInt() + 11
            )
            assertEquals(expectedParent, parent)
            assertTrue(partialInfo.isVisibleToUser)
            val location = IntArray(2); view.getLocationOnScreen(location)
            assertEquals(Rect(expectedParent).apply { offset(location[0], location[1]) }, screen)
            assertTrue(provider.performAction(partialId, AccessibilityNodeInfo.ACTION_CLICK, null))
            assertEquals(listOf("https://cell-one.example"), activated)

            assertTrue(tap(view, partialBounds.centerX() + 7f, partialBounds.centerY() + 11f))
            assertEquals(listOf("https://cell-one.example", "https://cell-one.example"), activated)
            val outsideClipX = (rawPartialBounds.left + partial.clip.left) / 2f
            assertTrue(rawPartialBounds.contains(outsideClipX, partialBounds.centerY()))
            assertFalse(partial.clip.contains(outsideClipX, partialBounds.centerY()))
            assertFalse(tap(view, outsideClipX + 7f, partialBounds.centerY() + 11f))
            assertEquals(listOf("https://cell-one.example", "https://cell-one.example"), activated)

            val fullyClippedOffset = first.bounds.right - tableBounds.left + 1f
            assertTrue(fullyClippedOffset <= surface.bounds.width() - surface.hostViewportWidth)
            val hiddenOwner = ViewerTablePresentationOwner().apply { setLogicalOffset(fullyClippedOffset, surface) }
            val hiddenSnapshot = ViewerTablePresentation.project(layout, hiddenOwner, ViewerTablePresentationViewport.Unknown)
            val hidden = hiddenSnapshot.accessibilityNodes.first { hrefOf(it) == "https://cell-one.example" }
            assertFalse(RectF(hidden.bounds).intersect(hidden.clip))
            view.setTableLogicalOffset(surface.identity, fullyClippedOffset)
            val hiddenId = idFor(hiddenSnapshot, "https://cell-one.example")
            val hiddenInfo = requireNotNull(provider.createAccessibilityNodeInfo(hiddenId))
            val hiddenBounds = Rect(); hiddenInfo.getBoundsInParent(hiddenBounds)
            assertTrue(hiddenBounds.isEmpty)
            assertFalse(hiddenInfo.isVisibleToUser)
            assertFalse(provider.performAction(hiddenId, AccessibilityNodeInfo.ACTION_CLICK, null))
            assertFalse(provider.performAction(hiddenId, AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null))
            assertEquals(listOf("https://cell-one.example", "https://cell-one.example"), activated)
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `compiler backed nested table interactions retain source and clear focus`() {
        val layout = prepare(
            """
            {"type":"doc","content":[
              {"type":"paragraph","content":[{"type":"text","text":"before","marks":[{"type":"link","attrs":{"href":"https://before.example"}}]}]},
              {"type":"table","content":[{"type":"table_row","content":[
                {"type":"table_cell","attrs":{"colwidth":[300]},"content":[
                  {"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"link","attrs":{"href":"https://cell-one.example"}}]}]},
                  {"type":"table","content":[{"type":"table_row","content":[
                    {"type":"table_cell","attrs":{"colwidth":[300]},"content":[
                      {"type":"paragraph","content":[
                        {"type":"mention","attrs":{"id":"nested-mention","label":"Ada"}},
                        {"type":"text","text":"nested","marks":[{"type":"link","attrs":{"href":"https://nested.example"}}]}
                      ]}
                    ]}
                  ]}]}
                ]},
                {"type":"table_cell","attrs":{"colwidth":[300]},"content":[
                  {"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"link","attrs":{"href":"https://cell-two.example"}}]}]}
                ]}
              ]}]},
              {"type":"paragraph","content":[{"type":"text","text":"after","marks":[{"type":"link","attrs":{"href":"https://after.example"}}]}]}
            ]}
            """.trimIndent(),
            config = interactionConfig()
        )

        withMountedDrawing(layout) { view ->
            view.mentionInteractionsEnabled = true
            val initial = ViewerTablePresentation.project(
                layout,
                ViewerTablePresentationOwner(),
                ViewerTablePresentationViewport.Unknown
            )
            fun interaction(node: ViewerTablePresentedAccessibilityNode) =
                requireNotNull(node.node.interactionIndex.let { node.layout.interactions.getOrNull(it) })
            fun href(node: ViewerTablePresentedAccessibilityNode) = interaction(node).href
            fun nodeForHref(snapshot: ViewerTablePresentationSnapshot, targetHref: String) =
                snapshot.accessibilityNodes.first { href(it) == targetHref }
            fun idFor(snapshot: ViewerTablePresentationSnapshot, node: ViewerTablePresentedAccessibilityNode) =
                snapshot.accessibilityNodes.indexOf(node) + 1
            fun tap(node: ViewerTablePresentedAccessibilityNode): Boolean =
                tap(view, node.bounds.centerX() + 7f, node.bounds.centerY() + 11f)

            val rootBefore = nodeForHref(initial, "https://before.example")
            val rootAfter = nodeForHref(initial, "https://after.example")
            val cellOne = nodeForHref(initial, "https://cell-one.example")
            val cellTwo = nodeForHref(initial, "https://cell-two.example")
            val nestedLink = nodeForHref(initial, "https://nested.example")
            val nestedMention = initial.accessibilityNodes.single { it.node.role == PreparedProseAccessibilityNode.Role.MENTION }
            val mentionInteraction = interaction(nestedMention)

            assertEquals("Ada", mentionInteraction.label)
            assertEquals("{\"id\":\"nested-mention\",\"label\":\"Ada\"}", mentionInteraction.attrsJson)
            // before(8) + outer opens(3) + same(6) + nested chrome(4)
            assertEquals(21L, mentionInteraction.docPos)
            assertEquals("same", interaction(cellOne).visibleText)
            assertEquals("same", interaction(cellTwo).visibleText)
            assertTrue(cellOne.sourceIdentity != cellTwo.sourceIdentity)

            val activated = mutableListOf<PreparedProseInteraction>()
            view.onInteractionActivated = { activated += it; true }
            assertTrue(tap(rootBefore))
            assertTrue(tap(rootAfter))
            assertTrue(tap(cellOne))
            assertTrue(tap(nestedLink))
            assertTrue(tap(nestedMention))
            assertEquals(
                listOf(
                    "https://before.example",
                    "https://after.example",
                    "https://cell-one.example",
                    "https://nested.example",
                    null
                ),
                activated.map { it.href }
            )
            assertEquals(requireNotNull(mentionInteraction.docPos), activated.last().docPos)
            assertEquals(mentionInteraction.attrsJson, activated.last().attrsJson)

            val provider = view.accessibilityNodeProvider
            val nestedMentionId = idFor(initial, nestedMention)
            assertTrue(provider.performAction(nestedMentionId, AccessibilityNodeInfo.ACTION_CLICK, null))
            assertEquals(21L, activated.last().docPos)
            assertEquals(mentionInteraction.attrsJson, activated.last().attrsJson)
            val cellOneId = idFor(initial, cellOne)
            val activationCountBeforeCapabilities = activated.size
            view.linkInteractionsEnabled = false
            assertFalse(tap(rootBefore))
            assertEquals(activationCountBeforeCapabilities, activated.size)
            assertEquals(1, requireNotNull(provider.createAccessibilityNodeInfo(android.view.View.NO_ID)).childCount)
            assertEquals("Ada", requireNotNull(provider.createAccessibilityNodeInfo(1)).contentDescription)
            view.linkInteractionsEnabled = true
            view.mentionInteractionsEnabled = false
            assertFalse(tap(nestedMention))
            assertEquals(activationCountBeforeCapabilities, activated.size)
            assertEquals(5, requireNotNull(provider.createAccessibilityNodeInfo(android.view.View.NO_ID)).childCount)
            view.mentionInteractionsEnabled = true
            assertTrue(tap(nestedMention))
            assertTrue(provider.performAction(cellOneId, AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null))
            assertTrue(requireNotNull(provider.createAccessibilityNodeInfo(cellOneId)).isAccessibilityFocused)

            val surface = layout.blocks.first { it.tableSurface != null }.tableSurface!!
            view.setTableLogicalOffset(surface.identity, surface.bounds.width())
            val revealed = ViewerTablePresentation.project(
                layout,
                ViewerTablePresentationOwner().apply { setLogicalOffset(surface.bounds.width(), surface) },
                ViewerTablePresentationViewport.Unknown
            )
            val revealedCellTwo = nodeForHref(revealed, "https://cell-two.example")
            assertTrue(RectF(revealedCellTwo.bounds).intersect(revealedCellTwo.clip))
            assertTrue(tap(revealedCellTwo))
            assertEquals("https://cell-two.example", activated.last().href)
            assertFalse(requireNotNull(provider.createAccessibilityNodeInfo(cellOneId)).isAccessibilityFocused)

            val revealedCellTwoId = idFor(revealed, revealedCellTwo)
            assertTrue(provider.performAction(revealedCellTwoId, AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null))
            assertTrue(requireNotNull(provider.createAccessibilityNodeInfo(revealedCellTwoId)).isAccessibilityFocused)
            view.visibility = android.view.View.INVISIBLE
            assertFalse(requireNotNull(provider.createAccessibilityNodeInfo(revealedCellTwoId)).isAccessibilityFocused)
            assertFalse(provider.performAction(revealedCellTwoId, AccessibilityNodeInfo.ACTION_CLICK, null))
            assertFalse(provider.performAction(revealedCellTwoId, AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS, null))
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `actual drawing view preserves nested header paint image identity and root prose`() {
        val layout = prepare(
            nestedHeaderImageSource(),
            """{"version":1,"styles":{"blockquote":{"backgroundColor":"#00ff00ff"}}}"""
        )
        val table = layout.blocks.first { it.tableSurface != null }
        val surface = table.tableSurface!!
        val image = layout.imageAttachments.single { it.source == "https://example.test/nested.png" }
        val imagePixels = Bitmap.createBitmap(20, 20, Bitmap.Config.ARGB_8888).apply {
            eraseColor(0)
            repeat(10) { y ->
                repeat(10) { x -> setPixel(x, y, 0xffff0000.toInt()) }
            }
            for (y in 10 until 20) {
                for (x in 10 until 20) setPixel(x, y, 0xff0000ff.toInt())
            }
        }
        withMountedDrawing(
            layout,
            width = layout.widthPx,
            height = layout.heightPx.coerceAtLeast(1),
            contentOriginXPx = 0,
            contentOriginYPx = 0
        ) { drawing ->
            assertEquals(0, viewerInputSurfaceCount(drawing))
            val lease = requireNotNull(
                DecodedBitmapBudget(64 * 1024).reserve(
                    imagePixels.allocationByteCount.toLong(),
                    DecodedBitmapPriority.VISIBLE
                )?.commit(imagePixels, imagePixels.allocationByteCount.toLong())
            )
            drawing.putImageLease(image.id, lease)
            val rendered = Bitmap.createBitmap(
                layout.widthPx,
                layout.heightPx.coerceAtLeast(1),
                Bitmap.Config.ARGB_8888
            )
            drawing.draw(Canvas(rendered))
            assertEquals(0, viewerInputSurfaceCount(drawing))

            val snapshot = ViewerTablePresentation.project(
                layout,
                ViewerTablePresentationOwner(),
                ViewerTablePresentationViewport.Unknown
            )
            val header = surface.cells.first { it.isHeader }
            val nestedHeader = snapshot.cells.single { it.surface !== surface && it.cell.isHeader }
            val projectedImage = snapshot.images.single { it.attachment.id == image.id }
            val quote = snapshot.blocks.asSequence().mapNotNull { presented ->
                presented.block.fragments.firstOrNull {
                    it.kind == PreparedProseFragmentKind.BACKGROUND &&
                        it.box?.backgroundColor == 0xff00ff00.toInt()
                }?.decorationBounds?.let { RectF(it).apply { offset(presented.originX, presented.originY) } }
            }.first()
            val rootProse = layout.blocks.first { it !== table }
            val prose = rootProse.fragments.first { it.kind == PreparedProseFragmentKind.TEXT }.bounds

            assertTrue("the table must start below root prose", table.tableBounds!!.top > 0)
            assertEquals(
            surface.style.headerBackgroundColor,
            rendered.getPixel(table.tableBounds!!.left + header.frame.left.toInt() + 2, table.tableBounds!!.top + header.frame.top.toInt() + 2)
        )
            assertEquals(
            nestedHeader.surface.style.headerBackgroundColor,
            rendered.getPixel(nestedHeader.bounds.left.toInt() + 2, nestedHeader.bounds.top.toInt() + 2)
        )
            assertEquals(0xff00ff00.toInt(), rendered.getPixel(quote.right.toInt() - 2, quote.top.toInt() + 2))
            assertEquals(0xffff0000.toInt(), imagePixels.getPixel(4, 4))
            assertEquals(0xff0000ff.toInt(), imagePixels.getPixel(15, 15))
            assertEquals(
            imagePixels.getPixel(4, 4),
            rendered.getPixel(projectedImage.bounds.left.toInt() + 4, projectedImage.bounds.top.toInt() + 4)
        )
            assertEquals(
            imagePixels.getPixel(15, 15),
            rendered.getPixel(projectedImage.bounds.left.toInt() + 15, projectedImage.bounds.top.toInt() + 15)
        )
            assertTrue(
            "root prose ink must survive table drawing",
            (prose.top until prose.bottom).any { y ->
                (prose.left until prose.right).any { x ->
                    rendered.getPixel(x, y).let { pixel ->
                        pixel ushr 24 > 0 &&
                            (pixel ushr 16 and 0xff) < 200 &&
                            (pixel ushr 8 and 0xff) < 200 &&
                            (pixel and 0xff) < 200
                    }
                }
            }
        )
            assertEquals(1, layout.imageAttachments.count { it.id == image.id })
            assertEquals(20, image.declaredSize?.first)
            assertEquals(20, image.declaredSize?.second)
            drawing.setTableLogicalOffset(surface.identity, surface.bounds.width())
            drawing.draw(Canvas(rendered))
            assertEquals(0, viewerInputSurfaceCount(drawing))
            drawing.setTableLogicalOffset(surface.identity, 0f)
            drawing.draw(Canvas(rendered))
            assertEquals(0, viewerInputSurfaceCount(drawing))
            drawing.clearImageLeases()
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `mounted table offsets clip image paint and refresh manager image eligibility`() {
        val sourcePixels = Bitmap.createBitmap(20, 20, Bitmap.Config.ARGB_8888).apply {
            eraseColor(0)
            repeat(10) { y -> repeat(10) { x -> setPixel(x, y, 0xffff0000.toInt()) } }
            for (y in 10 until 20) for (x in 10 until 20) setPixel(x, y, 0xff0000ff.toInt())
        }
        val sourceBytes = java.io.ByteArrayOutputStream().use { output ->
            check(sourcePixels.compress(Bitmap.CompressFormat.PNG, 100, output))
            output.toByteArray()
        }
        val localSource = "data:image/png;base64," + android.util.Base64.encodeToString(sourceBytes, android.util.Base64.NO_WRAP)
        val localLayout = prepare(
            nestedHeaderImageSource(
                imageSource = localSource,
                secondColumnWidth = 500,
                nestedSecondColumnWidth = 500
            )
        )
        val localTable = localLayout.blocks.first { it.tableSurface != null }
        val localSurface = localTable.tableSurface!!
        val localImage = localLayout.imageAttachments.single()
        val contentOriginX = 140
        val contentOriginY = 11
        val drawing = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        drawing.install(localLayout, contentOriginXPx = contentOriginX, contentOriginYPx = contentOriginY)
        drawing.layout(0, 0, localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY)
        val state = PreparedProseViewerManager.ViewState()
        state.imagePipeline.begin("table-offset", imagesEnabled = true)
        val firstAcquired = java.util.concurrent.CountDownLatch(1)
        val returnedAcquired = java.util.concurrent.CountDownLatch(1)
        val acquisitionCount = java.util.concurrent.atomic.AtomicInteger()
        val released = mutableSetOf<String>()
        state.imagePipeline.onPixels = { attachment, lease ->
            drawing.putImageLease(attachment.id, lease)
            if (acquisitionCount.incrementAndGet() == 1) firstAcquired.countDown() else returnedAcquired.countDown()
        }
        state.imagePipeline.onPixelsReleased = { ids ->
            released += ids
            drawing.removeImageLeases(ids)
        }
        var delivered = emptyList<ViewerImageAttachment>()
        drawing.onVisibleImagesChanged = { visible, attachments ->
            delivered = attachments
            state.requestVisibleImages(visible, attachments)
        }
        val initialSnapshot = ViewerTablePresentation.project(
            localLayout,
            ViewerTablePresentationOwner(),
            ViewerTablePresentationViewport.Unknown
        )
        val initial = initialSnapshot.images.single { it.attachment.id == localImage.id }
        val nestedSurface = initialSnapshot.cells.first { it.surface !== localSurface }.surface
        val nestedHostLeft = initialSnapshot.cells.first { it.surface === nestedSurface }.clip.left
        assertTrue(nestedHostLeft > 0f)

        drawing.draw(Canvas(Bitmap.createBitmap(localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY, Bitmap.Config.ARGB_8888)))
        assertEquals(listOf(localImage.id), delivered.map { it.id })
        assertEquals(1, state.imagePipeline.requestCountForTesting)
        assertTrue(drainMainUntil(firstAcquired))

        val fullyClippedOffset = initial.bounds.right - nestedHostLeft + 1f
        assertTrue("expected overflowing table", fullyClippedOffset > 0f)
        assertTrue(fullyClippedOffset <= nestedSurface.bounds.width() - nestedSurface.hostViewportWidth)
        drawing.setTableLogicalOffset(nestedSurface.identity, fullyClippedOffset)
        drawing.draw(Canvas(Bitmap.createBitmap(localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY, Bitmap.Config.ARGB_8888)))
        assertTrue(delivered.isEmpty())
        assertEquals(1, state.imagePipeline.requestCountForTesting)
        assertEquals(setOf(localImage.id), released)

        val partialOffset = initial.bounds.left - nestedHostLeft + initial.bounds.width() / 2f
        drawing.setTableLogicalOffset(nestedSurface.identity, partialOffset)
        val partialOwner = ViewerTablePresentationOwner().apply {
            setLogicalOffset(partialOffset, nestedSurface)
        }
        val partial = ViewerTablePresentation.project(
            localLayout,
            partialOwner,
            ViewerTablePresentationViewport.Unknown
        ).images.single { it.attachment.id == localImage.id }
        val visiblePartial = RectF(partial.bounds)
        assertTrue(visiblePartial.intersect(partial.clip))
        assertTrue(visiblePartial.width() in 1f..<partial.bounds.width())
        val partiallyClipped = Bitmap.createBitmap(localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY, Bitmap.Config.ARGB_8888)
        drawing.draw(Canvas(partiallyClipped))

        assertEquals(listOf(localImage.id), delivered.map { it.id })
        assertEquals(2, state.imagePipeline.requestCountForTesting)
        assertTrue(drainMainUntil(returnedAcquired))
        partiallyClipped.eraseColor(0)
        drawing.draw(Canvas(partiallyClipped))
        val sourcePointX = (initial.bounds.left + initial.bounds.width() * 0.75f - partialOffset + contentOriginX).toInt()
        val sourcePointY = (initial.bounds.top + initial.bounds.height() * 0.75f + contentOriginY).toInt()
        assertEquals(sourcePixels.getPixel(15, 15), partiallyClipped.getPixel(sourcePointX, sourcePointY))
        val withoutImage = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        withoutImage.install(localLayout, contentOriginXPx = contentOriginX, contentOriginYPx = contentOriginY)
        withoutImage.layout(0, 0, localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY)
        withoutImage.setTableLogicalOffset(nestedSurface.identity, partialOffset)
        val baseline = Bitmap.createBitmap(localLayout.widthPx + contentOriginX, localLayout.heightPx.coerceAtLeast(1) + contentOriginY, Bitmap.Config.ARGB_8888)
        withoutImage.draw(Canvas(baseline))
        val outsideClipX = (initial.bounds.left + initial.bounds.width() * 0.45f - partialOffset + contentOriginX).toInt()
        val outsideClipY = (initial.bounds.top + initial.bounds.height() * 0.2f + contentOriginY).toInt()
        assertTrue(outsideClipX >= contentOriginX)
        assertTrue(outsideClipX < partial.clip.left + contentOriginX)
        assertEquals(baseline.getPixel(outsideClipX, outsideClipY), partiallyClipped.getPixel(outsideClipX, outsideClipY))
        assertEquals(20 to 20, delivered.single().declaredSize)
        assertEquals(localImage.ordinal, delivered.single().ordinal)
        assertEquals(localImage.id, delivered.single().id)

        state.imagePipeline.cancel()
        drawing.clearImageLeases()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `actual drawing view paints compiler table chrome and keeps mounted offsets independent`() {
        val layout = prepare(
            """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"head"}]}]},{"type":"table_cell","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"body"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"""
        )
        val table = layout.blocks.first { it.tableSurface != null }
        val surface = table.tableSurface!!
        val first = surface.cells.first()
        val context = RuntimeEnvironment.getApplication()
        val left = PreparedProseDrawingView(context)
        val right = PreparedProseDrawingView(context)
        var geometryChanges = 0
        left.onTableGeometryChanged = { geometryChanges += 1 }
        listOf(left, right).forEach { view ->
            view.install(layout)
            view.layout(0, 0, layout.widthPx, layout.heightPx.coerceAtLeast(1))
        }

        val before = Bitmap.createBitmap(layout.widthPx, layout.heightPx.coerceAtLeast(1), Bitmap.Config.ARGB_8888)
        left.draw(Canvas(before))
        val header = surface.cells.first { it.isHeader }
        val frame = table.tableBounds!!
        assertEquals(surface.style.headerBackgroundColor, before.getPixel(
            frame.left + header.frame.left.toInt() + 2,
            frame.top + header.frame.top.toInt() + 2
        ))
        assertTrue("adjacent prose remains in the same prepared artifact", layout.blocks.any { it !== table && it.fragments.isNotEmpty() })

        assertTrue(surface.bounds.width() > surface.hostViewportWidth)
        val headerX = frame.left + header.frame.left.toInt() + 100
        val headerY = frame.top + header.frame.top.toInt() + 2
        left.setTableLogicalOffset(surface.identity, surface.bounds.width())
        assertEquals(1, geometryChanges)
        assertTrue(left.tablePhysicalOffsetForTesting(surface.identity) > 0f)
        val shifted = Bitmap.createBitmap(layout.widthPx, layout.heightPx.coerceAtLeast(1), Bitmap.Config.ARGB_8888)
        left.draw(Canvas(shifted))
        val unchanged = Bitmap.createBitmap(layout.widthPx, layout.heightPx.coerceAtLeast(1), Bitmap.Config.ARGB_8888)
        right.draw(Canvas(unchanged))
        assertTrue("left offset must move header paint", shifted.getPixel(headerX, headerY) != surface.style.headerBackgroundColor)
        assertEquals(0f, right.tablePhysicalOffsetForTesting(surface.identity), 0f)
        assertEquals("right mounted owner keeps its own offset", surface.style.headerBackgroundColor, unchanged.getPixel(headerX, headerY))
        val projected = ViewerTablePresentation.project(layout, ViewerTablePresentationOwner(), ViewerTablePresentationViewport.Unknown)
        val sourcePoint = projected.cells.first { it.sourcePosition == first.sourcePosition }.contentBounds
        assertTrue(sourcePoint.width() > 0f && sourcePoint.height() > 0f)
    }
    @Test
    fun `compiler backed admission counts flat table cells before layout preparation`() {
        val admittedEngine = CountingAdmissionLayoutEngine()
        val admitted = PreparedProseLayoutRegistry(
            compiler = ::compileWithRust,
            layoutEngine = admittedEngine
        ).measure(
            ProseViewerRequest(
                ProseViewerSource.Json(imageTableSource(ViewerImageAttachment.MAXIMUM_ADMITTED_ATTACHMENTS)),
                ProseViewerConfiguration(CONFIG, imagesEnabled = true)
            ),
            widthPx = 320,
            density = 1f
        )
        assertNull(admitted.error)
        assertEquals(1, admittedEngine.preparationCount)

        val rejectedEngine = CountingAdmissionLayoutEngine()
        val rejected = PreparedProseLayoutRegistry(
            compiler = ::compileWithRust,
            layoutEngine = rejectedEngine
        ).measure(
            ProseViewerRequest(
                ProseViewerSource.Json(imageTableSource(ViewerImageAttachment.MAXIMUM_ADMITTED_ATTACHMENTS + 1)),
                ProseViewerConfiguration(CONFIG, imagesEnabled = true)
            ),
            widthPx = 320,
            density = 1f
        )
        assertEquals("ATTACHMENT_LIMIT_EXCEEDED", rejected.error?.code?.value)
        assertEquals(0, rejectedEngine.preparationCount)
    }

    @Test
    fun `compiler backed raised depth 110 tables prepare finite retained surfaces`() {
        val layout = prepare(
            nestedTablesSource(110),
            config = CONFIG.dropLast(1) + ",\"limits\":{\"resource\":{\"maxDocumentDepth\":1024}}}"
        )

        assertNull(layout.error)
        assertTrue(layout.widthPx > 0 && layout.heightPx >= 0)
        var prepared = layout
        var surfaceCount = 0
        while (prepared.blocks.firstOrNull()?.tableSurface != null) {
            val surface = prepared.blocks.first().tableSurface!!
            surfaceCount += 1
            assertTrue(surface.layout.contentWidth.isFinite() && surface.layout.contentHeight.isFinite())
            prepared = surface.cells.single().content
        }
        assertEquals(110, surfaceCount)
    }

    @Test
    fun `compiler backed identical atom cells measure each source height into the shared row`() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"card"}]},{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"card"}]}]}]}]}"""
        val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG)))
        val table = document.blocks.single().table!!
        val atomPositions = table.cells.map { cell ->
            (document.cellDocument(cell).blocks.single().inlines.single() as ViewerInline.Atom).docPos
        }

        assertEquals(1, table.cells.map { it.contentKey }.distinct().size)
        assertEquals(2, atomPositions.distinct().size)

        val theme = """{"viewerAtoms":{"generation":"table-atoms","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":40},"measurements":{"${atomPositions[0]}":{"width":82,"height":20},"${atomPositions[1]}":{"width":82,"height":100}}}}"""
        val key = ProseLayoutKey(document.semanticKey, 320, "table-atoms", 0, 0, 1L, 0, "table-atoms")
        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(
            document, key, PreparedProseTheme.resolve(theme, 1f), 320, 1f, false
        )
        val surface = layout.blocks.single().tableSurface!!
        val chrome = 2f * (TableStyle().cellPadding + TableStyle().borderWidth)

        assertEquals(listOf(20, 100), surface.cells.map { it.content.viewerAtoms.single().bounds.height() })
        surface.cells.forEach { cell ->
            assertTrue(cell.frame.height >= cell.content.heightPx + chrome)
        }
        assertTrue(surface.cells.single { it.content.heightPx == 100 }.frame.height >= 100 + chrome)
    }

    @Test
    fun `compiler backed table atom dispatch reprojects offset with complete finite metadata`() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"card"}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"card"}]}]}]}]}"""
        val theme = """{"viewerAtoms":{"generation":"table-events","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":40}}}"""
        val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, themeJson = theme)))
        val expectedAtoms = document.blocks.single().table!!.cells.map { cell ->
            document.cellDocument(cell).blocks.single().inlines.single() as ViewerInline.Atom
        }
        val key = ProseLayoutKey(document.semanticKey, 320, "events", 0, 0, 1L, 0, "events")
        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(document, key, PreparedProseTheme.resolve(theme, 1f), 320, 1f, false)
        val surface = layout.blocks.single().tableSurface!!
        val manager = PreparedProseViewerManager()
        val view = PreparedProseDrawingView(RuntimeEnvironment.getApplication())
        val state = PreparedProseViewerManager.ViewState(source = source, themeJson = theme).apply {
            revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 91)
        }
        val token = FabricSurfaceToken(73, 21)
        PreparedProseLayoutRegistry.shared.registerFabricLease(token, 91)
        val generation = state.adopt(token, state.requestOrNull()!!)
        view.install(layout)
        state.installAtomArtifact(generation, layout)
        val states = PreparedProseViewerManager::class.java.getDeclaredField("states").apply { isAccessible = true }
            .get(manager) as MutableMap<PreparedProseDrawingView, PreparedProseViewerManager.ViewState>
        states[view] = state
        val events = mutableListOf<ViewerAtomLayoutEvent>()
        manager.atomLayoutEventSinkForTesting = events::add
        val dispatch = PreparedProseViewerManager::class.java.getDeclaredMethod("dispatchAtomLayout", PreparedProseDrawingView::class.java, PreparedProseViewerManager.ViewState::class.java, Class.forName("com.apollohg.editor.viewer.PreparedMountTicket")).apply { isAccessible = true }
        view.onTableGeometryChanged = { dispatch.invoke(manager, view, state, null) }
        dispatch.invoke(manager, view, state, null)
        view.setTableLogicalOffset(surface.identity, 800f)
        assertEquals(2, events.size)
        val first = JSONObject(events[0].atomsJson)
        val second = JSONObject(events[1].atomsJson)
        assertTrue(second.getLong("presentationSequence") > first.getLong("presentationSequence"))
        listOf(first, second).forEachIndexed { eventIndex, envelope ->
            assertEquals("table-events", events[eventIndex].generation)
            assertEquals("1", events[eventIndex].revision)
            val atoms = envelope.getJSONArray("atoms")
            assertEquals(2, atoms.length())
            assertEquals(expectedAtoms.map { it.docPos }, List(atoms.length()) { atoms.getJSONObject(it).getLong("docPos") })
            assertEquals(expectedAtoms.map { it.attrsJson }, List(atoms.length()) { atoms.getJSONObject(it).getString("attrsJson") })
            repeat(atoms.length()) { index ->
                val atom = atoms.getJSONObject(index)
                assertTrue(atom.getLong("docPos") > 0)
                assertTrue(atom.getString("attrsJson").isNotEmpty())
                assertEquals(surface.cells[index].content.viewerAtoms.single().bounds.width().toDouble(), atom.getDouble("width"), 0.0)
                val clip = atom.getJSONObject("presentation").getJSONObject("clip")
                listOf("x", "y", "width", "height").forEach { keyName -> assertTrue(clip.getDouble(keyName).isFinite()) }
            }
        }
        assertTrue(first.getJSONArray("atoms").getJSONObject(0).getDouble("x") != second.getJSONArray("atoms").getJSONObject(0).getDouble("x"))
        PreparedProseLayoutRegistry.shared.deactivateFabricLease(token, 91)
        state.release()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `manager callbacks retain table atoms across viewport and owner changes`() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"card"}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"card"}]}]}]}]}"""
        val theme = """{"viewerAtoms":{"generation":"table-events","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":40}}}"""
        var preparations = 0
        val engine = StaticLayoutAndroidProseLayoutEngine().apply { tableCellPreparationObserver = { preparations += 1 } }
        val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, themeJson = theme)))
        val layout = prepare(document, theme, engine)
        val surface = requireNotNull(layout.blocks.single().tableSurface)
        val manager = PreparedProseViewerManager()
        val events = mutableListOf<ViewerAtomLayoutEvent>()
        manager.atomLayoutEventSinkForTesting = events::add
        val token = FabricSurfaceToken(74, 22)
        val registry = PreparedProseLayoutRegistry.shared
        withMountedDrawing(layout, width = 120, height = 80, viewFactory = { activity ->
            val context = ThemedReactContext(BridgeReactContext(activity), activity, "tables", 74)
            PreparedProseViewerManager::class.java.getDeclaredMethod("createViewInstance", ThemedReactContext::class.java)
                .apply { isAccessible = true }.invoke(manager, context) as PreparedProseDrawingView
        }) { view ->
            @Suppress("UNCHECKED_CAST")
            val states = PreparedProseViewerManager::class.java.getDeclaredField("states")
                .apply { isAccessible = true }.get(manager) as Map<PreparedProseDrawingView, PreparedProseViewerManager.ViewState>
            val state = requireNotNull(states[view])
            state.source = source
            state.configJson = CONFIG
            state.themeJson = theme
            state.revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 91)
            registry.registerFabricLease(token, 91)
            val install = PreparedProseViewerManager::class.java.getDeclaredMethod(
                "installPreparedTicket", PreparedProseDrawingView::class.java,
                PreparedProseViewerManager.ViewState::class.java, PreparedMountTicket::class.java
            ).apply { isAccessible = true }
            fun installCurrent(artifact: PreparedProseLayout = layout) {
                val generation = state.adopt(token, requireNotNull(state.requestOrNull()))
                install.invoke(manager, view, state, PreparedMountTicket(generation, 0, 320, 7, 11, 1f.toRawBits(), artifact))
            }
            fun paint() = view.draw(Canvas(Bitmap.createBitmap(120, 80, Bitmap.Config.ARGB_8888)))
            fun assertAtoms(candidates: List<Boolean>) {
                val event = events.last()
                assertEquals("table-events", event.generation)
                assertEquals("1", event.revision)
                val atoms = JSONObject(event.atomsJson).getJSONArray("atoms")
                assertEquals(2, atoms.length())
                assertEquals(listOf(3L, 6L), (0..1).map { atoms.getJSONObject(it).getLong("docPos") })
                repeat(2) { index ->
                    val atom = atoms.getJSONObject(index)
                    assertEquals("{}", atom.getString("attrsJson"))
                    assertEquals(582.0, atom.getDouble("width"), 0.0)
                    assertEquals(40.0, atom.getDouble("height"), 0.0)
                    val presentation = atom.getJSONObject("presentation")
                    assertEquals(candidates[index], presentation.getBoolean("candidate"))
                    val clip = presentation.getJSONObject("clip")
                    listOf("x", "y", "width", "height").forEach { name -> assertTrue(clip.getDouble(name).isFinite()) }
                }
            }
            try {
                installCurrent()
                assertEquals(2, preparations)
                assertAtoms(listOf(true, false))
                val initialCount = events.size
                paint()
                assertEquals(initialCount, events.size)
                val firstX = JSONObject(events.last().atomsJson).getJSONArray("atoms").getJSONObject(0).getDouble("x")
                view.setTableLogicalOffset(surface.identity, 800f)
                assertEquals(initialCount + 1, events.size)
                assertAtoms(listOf(false, true))
                assertEquals(firstX - 800, JSONObject(events.last().atomsJson).getJSONArray("atoms").getJSONObject(0).getDouble("x"), 0.0)
                view.visibility = android.view.View.INVISIBLE
                paint()
                assertAtoms(listOf(false, false))
                view.visibility = android.view.View.VISIBLE
                paint()
                assertAtoms(listOf(false, true))
                (view.parent as FrameLayout).removeView(view)
                paint()
                assertAtoms(listOf(true, true))
                assertEquals(2, preparations)
                assertSame(layout, view.preparedLayout)
                val callback = requireNotNull(view.onTableGeometryChanged)
                state.themeJson = theme.replace("\"revision\":\"1\"", "\"revision\":\"2\"")
                state.adopt(token, requireNotNull(state.requestOrNull()))
                val beforeReplacement = events.size
                callback()
                assertEquals(beforeReplacement, events.size)
                val replacement = prepare(document, state.themeJson, engine)
                installCurrent(replacement)
                assertEquals(beforeReplacement + 1, events.size)
                assertEquals("2", events.last().revision)
                assertSame(replacement, view.preparedLayout)
                registry.deactivateFabricLease(token, 91)
                view.setTableLogicalOffset(surface.identity, 400f)
                callback()
                assertEquals(beforeReplacement + 1, events.size)
                state.revisions = PreparedProseViewerManager.FabricStateRevisions(0, 0, 92)
                registry.registerFabricLease(token, 92)
                installCurrent(replacement)
                assertEquals(beforeReplacement + 2, events.size)
                val sequences = events.map { JSONObject(it.atomsJson).getLong("presentationSequence") }
                assertTrue(sequences.zipWithNext().all { (first, second) -> second > first })
                manager.onDropViewInstance(view)
                val beforeStale = events.size
                callback()
                assertEquals(beforeStale, events.size)
            } finally {
                registry.deactivateFabricLease(token, 91)
                registry.deactivateFabricLease(token, 92)
                state.release()
            }
        }
    }

    @Test
    fun `compiler backed tables scale declared and default minimum columns once`() {
        val source = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"""
        val minimumSource = """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"""

        listOf(1f to 150, 2f to 300).forEach { (density, widthPx) ->
            val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG)))
            val key = ProseLayoutKey(document.semanticKey, widthPx, "density", 0, 0, density.toRawBits().toLong(), 0, "density")
            val surface = StaticLayoutAndroidProseLayoutEngine().prepare(
                document, key, PreparedProseTheme.resolve(null, density), widthPx, density, false
            ).blocks.single().tableSurface!!
            val scale = density.toInt()

            assertEquals(listOf(100u, 100u), document.blocks.single().table!!.columnWidths)
            assertEquals(listOf(100f * scale, 100f * scale), surface.layout.columnWidths)
            assertEquals(200f * scale, surface.layout.contentWidth)
            assertTrue(surface.layout.contentWidth > widthPx)
            surface.cells.forEach { cell ->
                assertEquals(9 * scale, cell.contentOrigin.first)
                assertEquals(82 * scale, cell.content.widthPx)
                assertEquals(cell.content.widthPx, cell.content.key.widthPx)
            }

            val minimumDocument = compileWithRust(
                ProseViewerRequest(ProseViewerSource.Json(minimumSource), ProseViewerConfiguration(CONFIG))
            )
            val minimumKey = key.copy(semanticKey = minimumDocument.semanticKey)
            val minimum = StaticLayoutAndroidProseLayoutEngine().prepare(
                minimumDocument, minimumKey, PreparedProseTheme.resolve(null, density), widthPx, density, false
            ).blocks.single().tableSurface!!

            assertEquals(listOf(null, null), minimumDocument.blocks.single().table!!.columnWidths)
            assertEquals(listOf(80f * scale, 80f * scale), minimum.layout.columnWidths)
            assertEquals(160f * scale, minimum.layout.contentWidth)
            assertTrue(minimum.layout.contentWidth > widthPx)
            minimum.cells.forEach { cell ->
                assertEquals(9 * scale, cell.contentOrigin.first)
                assertEquals(62 * scale, cell.content.widthPx)
                assertEquals(cell.content.widthPx, cell.content.key.widthPx)
            }
        }
    }
    @Test
    fun `compiler backed bold identical cells retain distinct prepared artifacts and width keys`() {
        val layout = prepare("""{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"bold"}]}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same","marks":[{"type":"bold"}]}]}]}]}]}]}""")
        val cells = layout.blocks.single().tableSurface!!.cells
        val text = cells.first().content.blocks.flatMap { it.fragments }.first { it.kind == PreparedProseFragmentKind.TEXT }.layout!!.text as Spanned
        val style = text.getSpans(0, text.length, ResolvedTextStyleSpan::class.java).single()
        assertEquals(Typeface.BOLD, style.typeface.style)
        assertEquals(2, cells.size)
        assertTrue(cells.all { it.content.key.widthPx == it.content.widthPx && it.content.widthPx > 0 })
        assertTrue(cells[0].content.key.semanticKey != cells[1].content.key.semanticKey)
    }

    @Test
    fun `themed blockquote table retains one root content box and enclosing bounds`() {
        val source = """{"type":"doc","content":[{"type":"blockquote","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"inside"}]}]}]}]}]}]}"""
        val themeJson = """{"version":1,"styles":{"content":{"paddingLeft":20,"paddingRight":20,"backgroundColor":"#ff0000ff"},"blockquote":{"paddingLeft":11,"paddingRight":13,"backgroundColor":"#ffff00ff"}}}"""
        val document = compileWithRust(
            ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, themeJson = themeJson, imagesEnabled = true))
        )
        val key = ProseLayoutKey(document.semanticKey, 320, "table", 0, 0, 1L, 0, "table")
        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(
            document, key, PreparedProseTheme.resolve(themeJson, 1f), 320, 1f, false, key.semanticGenerationIdentity
        )
        val table = layout.blocks.single { it.tableSurface != null }
        val surface = table.tableSurface!!
        assertNotNull(layout.contentBox)
        assertNull(surface.cells.single().content.contentBox)
        val frame = table.tableBounds!!
        assertEquals(34, frame.left)
        val quote = layout.blocks.flatMap { it.fragments }.first { it.kind == PreparedProseFragmentKind.BACKGROUND && it.bounds.bottom >= frame.bottom }.bounds
        assertTrue(quote.left <= frame.left && quote.top <= frame.top && quote.right >= frame.right && quote.bottom >= frame.bottom)
        assertTrue(layout.retainedBytes >= document.retainedBytes + surface.retainedBytes)
    }

    @Test
    fun `irregular compiler table has finite source cells and padded parent image transform`() {
        val layout = prepare("""{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"image","attrs":{"src":"https://example.test/tall.png","width":20,"height":10}}]},{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}""")
        val table = layout.blocks.single()
        val surface = table.tableSurface!!
        assertEquals(3, surface.cells.size)
        assertTrue(surface.layout.contentWidth.isFinite() && surface.layout.contentHeight.isFinite())
        val cell = surface.cells.first()
        val local = cell.content.imageAttachments.single()
        val parent = layout.imageAttachments.single()
        val frame = table.tableBounds!!
        assertEquals(frame.left + cell.frame.left.toInt() + cell.contentOrigin.first + local.bounds.left, parent.bounds.left)
        assertEquals(frame.top + cell.frame.top.toInt() + cell.contentOrigin.second + local.bounds.top, parent.bounds.top)
    }

    @Test
    fun `root promotes nested code descriptors in document order once`() {
        val source = """{"type":"doc","content":[{"type":"codeBlock","attrs":{"language":"txt"},"content":[{"type":"text","text":"root-before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"codeBlock","attrs":{"language":"txt"},"content":[{"type":"text","text":"cell-before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"codeBlock","attrs":{"language":"txt"},"content":[{"type":"text","text":"nested"}]}]}]}]},{"type":"codeBlock","attrs":{"language":"txt"},"content":[{"type":"text","text":"cell-after"}]}]}]}]},{"type":"codeBlock","attrs":{"language":"txt"},"content":[{"type":"text","text":"root-after"}]}]}"""
        val document = compileWithRust(ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG)))
        val key = ProseLayoutKey(document.semanticKey, 320, "code", 0, 0, 1L, 0, "code")
        val theme = PreparedProseTheme.resolve(null, 1f).copy(codeHighlighting = NativeCodeHighlightingConfig("table-test", "one"))
        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(document, key, theme, 320, 1f, false)
        assertEquals(listOf("root-before", "cell-before", "nested", "cell-after", "root-after"), layout.codeHighlightBlocks.map { it.text })
        assertEquals(listOf(0, 1, 2, 3, 4), layout.codeHighlightBlocks.map { it.start })
    }

    @Test
    fun `compiler backed table flattens cell and nested images in document order`() {
        val source = """{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.test/outer.png","width":20,"height":10}},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"image","attrs":{"src":"https://example.test/cell.png","width":20,"height":10}},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"image","attrs":{"src":"https://example.test/nested.png","width":20,"height":10}}]}]}]},{"type":"image","attrs":{"src":"https://example.test/after.png","width":20,"height":10}}]}]}]}]}"""
        val document = compileWithRust(
            ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(CONFIG, imagesEnabled = true))
        )
        val key = ProseLayoutKey(document.semanticKey, 320, "table", 0, 0, 1L, 0, "table")

        val layout = StaticLayoutAndroidProseLayoutEngine().prepare(
            document, key, PreparedProseTheme.resolve(null, 1f), 320, 1f, false, key.semanticGenerationIdentity
        )

        assertEquals(
            listOf(
                "https://example.test/outer.png",
                "https://example.test/cell.png",
                "https://example.test/nested.png",
                "https://example.test/after.png"
            ),
            layout.imageAttachments.map { it.source }
        )
        assertEquals(listOf(0, 1, 2, 3), layout.imageAttachments.map { it.ordinal })
        assertTrue(layout.retainedBytes >= document.retainedBytes)
    }

    @Test
    fun `compiler backed table-only list siblings keep marker gutter and terminal spacing`() {
        val layout = prepare(
            """{"type":"doc","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]}]}]}]},{"type":"listItem","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}""",
            """{"list":{"itemSpacing":3,"spacingAfter":13}}"""
        )

        val tables = layout.blocks.filter { it.tableSurface != null }
        assertEquals(2, tables.size)
        assertEquals(3, tables[1].tableBounds!!.top - tables[0].tableBounds!!.bottom)
        assertEquals(13, layout.blocks[2].bounds.top - tables[1].tableBounds!!.bottom)
        tables.forEach { table ->
            val frame = table.tableBounds!!
            val marker = table.fragments.single { it.kind == PreparedProseFragmentKind.MARKER }
            assertEquals(table.tableSurface!!.layout.contentWidth.toInt(), frame.width())
            assertTrue(marker.bounds.right <= frame.left)
            assertTrue(frame.width() < 320)
        }
    }

    @Test
    fun `nested table-only list image uses inner table bounds during recursive parent promotion`() {
        val layout = prepare(
            """{
                "type":"doc",
                "content":[
                    {"type":"table","content":[
                        {"type":"table_row","content":[
                            {"type":"table_cell","content":[
                                {"type":"bulletList","content":[
                                    {"type":"listItem","content":[
                                        {"type":"bulletList","content":[
                                            {"type":"listItem","content":[
                                                {"type":"table","content":[
                                                    {"type":"table_row","content":[
                                                        {"type":"table_cell","content":[
                                                            {"type":"image","attrs":{"src":"https://example.test/nested-list.png","width":20,"height":10}}
                                                        ]}
                                                    ]}
                                                ]}
                                            ]}
                                        ]}
                                    ]}
                                ]}
                            ]}
                        ]}
                    ]}
                ]
            }""",
            """{"list":{"itemSpacing":3,"spacingAfter":13}}"""
        )

        val outerTable = layout.blocks.single { it.tableSurface != null }
        val outerFrame = outerTable.tableBounds!!
        val outerCell = outerTable.tableSurface!!.cells.single()
        val innerTable = outerCell.content.blocks.single { it.tableSurface != null }
        val innerFrame = innerTable.tableBounds!!
        val markers = innerTable.fragments.filter { it.kind == PreparedProseFragmentKind.MARKER }
        assertEquals(2, markers.size)
        assertTrue(markers.all { it.bounds.right <= innerFrame.left })
        assertTrue(innerTable.bounds.left < innerFrame.left)
        val innerCell = innerTable.tableSurface!!.cells.single()
        val local = innerCell.content.imageAttachments.single()
        val parent = layout.imageAttachments.single()
        assertEquals(listOf("https://example.test/nested-list.png"), layout.imageAttachments.map { it.source })
        assertEquals(listOf(local.id), layout.imageAttachments.map { it.id })
        assertEquals(listOf(0), layout.imageAttachments.map { it.ordinal })
        assertEquals(
            outerFrame.left + outerCell.frame.left.toInt() + outerCell.contentOrigin.first +
                innerFrame.left + innerCell.frame.left.toInt() + innerCell.contentOrigin.first + local.bounds.left,
            parent.bounds.left
        )
        assertEquals(
            outerFrame.top + outerCell.frame.top.toInt() + outerCell.contentOrigin.second +
                innerFrame.top + innerCell.frame.top.toInt() + innerCell.contentOrigin.second + local.bounds.top,
            parent.bounds.top
        )
    }

    @Test
    fun `compiler backed mounted presentation keeps full metadata and bounds offsets independently`() {
        val rtlConfig = CONFIG.replace(
            "\"class\":{\"default\":null}",
            "\"class\":{\"default\":null},\"dir\":{\"default\":null}"
        ).replace(
            "\"marks\":[{\"name\":\"bold\"}]",
            "\"marks\":[{\"name\":\"bold\"},{\"name\":\"link\",\"attrs\":{\"href\":{\"default\":\"\"}}}]"
        )
        fun node(type: String, content: List<Any> = emptyList(), attrs: Map<String, Any> = emptyMap()) =
            buildMap<String, Any> {
                put("type", type)
                if (content.isNotEmpty()) put("content", content)
                if (attrs.isNotEmpty()) put("attrs", attrs)
            }
        fun text(value: String, marks: List<Map<String, Any>> = emptyList()) = buildMap<String, Any> {
            put("type", "text")
            put("text", value)
            if (marks.isNotEmpty()) put("marks", marks)
        }
        fun cell(content: List<Any>) = node("table_cell", content, mapOf("colwidth" to listOf(100)))
        val image = node("image", attrs = mapOf("src" to "https://example.test/first.png", "width" to 20, "height" to 10))
        val nestedImage = node("image", attrs = mapOf("src" to "https://example.test/nested.png", "width" to 20, "height" to 10))
        val linkMark = mapOf("type" to "link", "attrs" to mapOf("href" to "https://example.test/link"))
        fun linked(label: String) = node("paragraph", listOf(text(label, listOf(linkMark + ("attrs" to mapOf("href" to "https://example.test/$label"))))))
        val nested = node("table", listOf(node("table_row", listOf(cell(listOf(nestedImage))))))
        val third = cell(listOf(node("paragraph", listOf(text("three")))))
        val fourth = cell(listOf(node("paragraph", listOf(text("four")))))
        val row = node("table_row", listOf(cell(listOf(image, node("card"), linked("cell"))), cell(listOf(nested)), third, fourth))
        val tableSource = node("table", listOf(row), mapOf("dir" to "rtl"))
        val source = JSONObject(node("doc", listOf(linked("before"), node("bulletList", listOf(node("listItem", listOf(tableSource)))), linked("after")))).toString()
        val theme = """{"viewerAtoms":{"generation":"presentation","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":20}}}"""
        val layout = prepare(source, theme, rtlConfig)
        val table = layout.blocks.first { it.tableSurface != null }
        val surface = table.tableSurface!!
        assertTrue(surface.isRightToLeft)
        assertTrue(surface.hostViewportWidth < surface.bounds.width())

        val firstOwner = ViewerTablePresentationOwner()
        val full = ViewerTablePresentation.project(layout, firstOwner, ViewerTablePresentationViewport.Unknown)
        assertEquals(surface.layout.sourceOrder, full.cells.filter { it.surface === surface }.map { it.sourcePosition })
        assertEquals(surface.cells.size, full.mountedCells.count { it.surface === surface })
        assertEquals(2, full.images.size)
        assertEquals(1, full.atoms.size)
        assertEquals(listOf("before", "cell", "after"), full.interactions.map { it.interaction.visibleText })
        assertTrue(full.accessibilityNodes.any { it.interactionSourceIdentity == full.interactions[1].sourceIdentity })
        val first = full.cells.first()
        assertEquals(first.clip.right, first.bounds.right)
        val nestedCell = full.cells.first { it.surface !== surface }
        val containingCell = full.cells.first { it.surface === surface && it.sourcePosition == surface.layout.sourceOrder[1] }
        val nestedBlock = full.blocks.first { it.layout === containingCell.content && it.block.tableSurface != null }
        val nestedSurface = nestedBlock.block.tableSurface!!
        val nestedFrame = nestedBlock.block.tableBounds!!
        val expectedNestedClip = RectF(
            maxOf(containingCell.clip.left, containingCell.contentBounds.left, containingCell.contentBounds.left + nestedFrame.left),
            maxOf(containingCell.clip.top, containingCell.contentBounds.top, containingCell.contentBounds.top + nestedFrame.top),
            minOf(containingCell.clip.right, containingCell.contentBounds.right, containingCell.contentBounds.left + nestedFrame.left + minOf(nestedSurface.hostViewportWidth, nestedSurface.bounds.width())),
            minOf(containingCell.clip.bottom, containingCell.contentBounds.bottom, containingCell.contentBounds.top + nestedFrame.top + nestedSurface.bounds.height())
        )
        assertEquals(expectedNestedClip, nestedCell.clip)

        val secondOwner = ViewerTablePresentationOwner()
        secondOwner.setLogicalOffset(Float.POSITIVE_INFINITY, surface)
        assertEquals(0f, secondOwner.logicalOffset(surface))
        secondOwner.setLogicalOffset(Float.MAX_VALUE, surface)
        val shifted = ViewerTablePresentation.project(layout, secondOwner, ViewerTablePresentationViewport.Unknown)
        val shiftedFirst = shifted.cells.first()
        assertEquals(surface.bounds.width() - surface.hostViewportWidth, shiftedFirst.bounds.right - first.bounds.right)
        assertEquals(0f, firstOwner.logicalOffset(surface))
        assertEquals(full.images.map { it.sourceIdentity }, shifted.images.map { it.sourceIdentity })
        assertEquals(full.images.map { it.bounds.width() to it.bounds.height() }, shifted.images.map { it.bounds.width() to it.bounds.height() })
        assertEquals(full.atoms.map { it.sourceIdentity }, shifted.atoms.map { it.sourceIdentity })
        assertEquals(full.atoms.map { it.bounds.width() to it.bounds.height() }, shifted.atoms.map { it.bounds.width() to it.bounds.height() })
        assertEquals(full.interactions.map { it.sourceIdentity }, shifted.interactions.map { it.sourceIdentity })
        val displacement = shiftedFirst.bounds.right - first.bounds.right
        full.images.zip(shifted.images).forEach { (before, after) -> assertEquals(displacement, after.bounds.left - before.bounds.left) }
        full.atoms.zip(shifted.atoms).forEach { (before, after) -> assertEquals(displacement, after.bounds.left - before.bounds.left) }
        val beforeCellLink = full.interactions.first { it.interaction.visibleText == "cell" }
        val afterCellLink = shifted.interactions.first { it.sourceIdentity == beforeCellLink.sourceIdentity }
        assertEquals(displacement, afterCellLink.rects.first().left - beforeCellLink.rects.first().left)

        val known = ViewerTablePresentation.project(
            layout,
            secondOwner,
            ViewerTablePresentationViewport.Known(Rect(table.tableBounds!!.left, table.tableBounds!!.top, table.tableBounds!!.left + 100, table.tableBounds!!.top + 100))
        )
        assertEquals(surface.layout.sourceOrder.takeLast(2), known.mountedCells.filter { it.surface === surface }.map { it.sourcePosition })
        assertEquals(0, ViewerTablePresentation.project(layout, secondOwner, ViewerTablePresentationViewport.Known(Rect())).mountedCells.size)
        assertEquals(0, ViewerTablePresentation.project(layout, secondOwner, ViewerTablePresentationViewport.Known(Rect(100_000, 100_000, 100_020, 100_020))).mountedCells.size)

        val drawing = PreparedProseDrawingView(RuntimeEnvironment.getApplication()).apply {
            install(layout)
        }
        val serialized = org.json.JSONArray(drawing.atomLayoutsJson(1f)).getJSONObject(0)
        val projectedAtom = full.atoms.single()
        assertEquals(projectedAtom.atom.docPos, serialized.getLong("docPos"))
        assertEquals(projectedAtom.bounds.width(), serialized.getDouble("width").toFloat())
        assertEquals(projectedAtom.bounds.height(), serialized.getDouble("height").toFloat())
        assertTrue(serialized.getJSONObject("presentation").getBoolean("candidate"))
        assertEquals(projectedAtom.clip.left, serialized.getJSONObject("presentation").getJSONObject("clip").getDouble("x").toFloat())
        drawing.setTableLogicalOffset(surface.identity, surface.bounds.width())
        val shiftedSerialized = org.json.JSONArray(drawing.atomLayoutsJson(1f)).getJSONObject(0)
        assertEquals(serialized.getDouble("width"), shiftedSerialized.getDouble("width"), 0.0)
        assertTrue(serialized.getDouble("x") != shiftedSerialized.getDouble("x"))

        val vertical = prepare("""{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}""")
        val verticalBlock = vertical.blocks.first { it.tableSurface != null }
        val verticalSurface = verticalBlock.tableSurface!!
        val middle = verticalSurface.cells[1]
        val verticalWindow = Rect(verticalBlock.tableBounds!!.left, verticalBlock.tableBounds!!.top + middle.frame.top.toInt(), verticalBlock.tableBounds!!.left + 20, verticalBlock.tableBounds!!.top + middle.frame.top.toInt() + middle.frame.height.toInt())
        val verticalSnapshot = ViewerTablePresentation.project(vertical, ViewerTablePresentationOwner(), ViewerTablePresentationViewport.Known(verticalWindow))
        assertEquals(verticalSurface.layout.sourceOrder.take(3), verticalSnapshot.mountedCells.map { it.sourcePosition })
    }

    @Test
    fun `compiler backed mounted presentation traverses admitted depth without dropping metadata`() {
        val layout = prepare(nestedTablesSource(110), config = CONFIG.dropLast(1) + ",\"limits\":{\"resource\":{\"maxDocumentDepth\":1024}}}")
        val snapshot = ViewerTablePresentation.project(layout, ViewerTablePresentationOwner(), ViewerTablePresentationViewport.Unknown)
        assertEquals(110, snapshot.cells.size)
        assertEquals(110, snapshot.mountedCells.size)
    }

    private fun drainMainUntil(latch: java.util.concurrent.CountDownLatch): Boolean {
        repeat(100) {
            shadowOf(Looper.getMainLooper()).idle()
            if (latch.await(10, java.util.concurrent.TimeUnit.MILLISECONDS)) return true
        }
        return false
    }

    private fun withMountedDrawing(
        layout: PreparedProseLayout,
        width: Int = 327,
        height: Int = layout.heightPx + 11,
        contentOriginXPx: Int = 7,
        contentOriginYPx: Int = 11,
        viewFactory: (Activity) -> PreparedProseDrawingView = { PreparedProseDrawingView(it) },
        block: (PreparedProseDrawingView) -> Unit
    ) {
        val controller = Robolectric.buildActivity(Activity::class.java)
        try {
            val activity = controller.create().get()
            shadowOf(activity.getSystemService(AccessibilityManager::class.java)).setEnabled(true)
            val host = FrameLayout(activity)
            activity.setContentView(host)
            val view = viewFactory(activity)
            view.install(
                layout,
                contentOriginXPx = contentOriginXPx,
                contentOriginYPx = contentOriginYPx
            )
            host.addView(view, FrameLayout.LayoutParams(width, height))
            val decor = activity.window.decorView
            decor.measure(
                android.view.View.MeasureSpec.makeMeasureSpec(width, android.view.View.MeasureSpec.EXACTLY),
                android.view.View.MeasureSpec.makeMeasureSpec(height, android.view.View.MeasureSpec.EXACTLY)
            )
            decor.layout(0, 0, width, height)
            host.measure(
                android.view.View.MeasureSpec.makeMeasureSpec(width, android.view.View.MeasureSpec.EXACTLY),
                android.view.View.MeasureSpec.makeMeasureSpec(height, android.view.View.MeasureSpec.EXACTLY)
            )
            host.layout(0, 0, width, height)
            view.layout(0, 0, width, height)
            controller.start().resume().visible().windowFocusChanged(true)
            shadowOf(Looper.getMainLooper()).idle()
            val global = Rect()
            assertTrue(view.isAttachedToWindow)
            assertNotNull(view.windowToken)
            assertEquals(android.view.View.VISIBLE, decor.windowVisibility)
            assertTrue(view.isShown)
            assertEquals(android.view.View.VISIBLE, view.windowVisibility)
            assertTrue(view.alpha > 0f)
            assertTrue(view.getGlobalVisibleRect(global))
            assertFalse(global.isEmpty)
            block(view)
        } finally {
            controller.pause().stop().destroy()
        }
    }

    private fun viewerInputSurfaceCount(root: View): Int {
        var count = 0
        fun visit(view: View) {
            if (view is EditText || view.onCheckIsTextEditor()) count += 1
            if (view is ViewGroup) {
                repeat(view.childCount) { visit(view.getChildAt(it)) }
            }
        }
        visit(root)
        return count
    }

    private fun tap(view: PreparedProseDrawingView, x: Float, y: Float): Boolean {
        val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(0, 1, MotionEvent.ACTION_UP, x, y, 0)
        return try {
            view.onTouchEvent(down) && view.onTouchEvent(up)
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private fun interactionConfig(): String = CONFIG
        .replace(
            "\"name\":\"text\",\"content\":\"\",\"group\":\"inline\",\"role\":\"text\"}",
            "\"name\":\"text\",\"content\":\"\",\"group\":\"inline\",\"role\":\"text\"},{\"name\":\"mention\",\"content\":\"\",\"group\":\"inline\",\"role\":\"inline\",\"isVoid\":true,\"allowUndeclaredAttrs\":true,\"attrs\":{\"label\":{\"default\":null}}}"
        )
        .replace(
            "\"marks\":[{\"name\":\"bold\"}]",
            "\"marks\":[{\"name\":\"bold\"},{\"name\":\"link\",\"attrs\":{\"href\":{}}}]"
        )

    private fun prepare(source: String, theme: String? = null, config: String = CONFIG) = compileWithRust(
        ProseViewerRequest(ProseViewerSource.Json(source), ProseViewerConfiguration(config, themeJson = theme, imagesEnabled = true))
    ).let { document ->
        prepare(document, theme)
    }

    private fun prepare(
        document: ViewerDocument,
        theme: String? = null,
        engine: StaticLayoutAndroidProseLayoutEngine = StaticLayoutAndroidProseLayoutEngine()
    ): PreparedProseLayout {
        val key = ProseLayoutKey(document.semanticKey, 320, "table", 0, 0, 1L, 0, "table")
        return engine.prepare(
            document,
            key,
            PreparedProseTheme.resolve(theme, 1f),
            320,
            1f,
            false,
            key.semanticGenerationIdentity
        )
    }

    private fun configWithGridSlots(slots: Int): String = JSONObject(CONFIG).apply {
        put("limits", JSONObject().put("resource", JSONObject().put("maxTableGridSlots", slots)))
    }.toString()

    private fun gridLimitSource(): String =
        """{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before ink"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"source survives"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after ink"}]}]}"""

    private fun imageTableSource(imageCount: Int): String {
        fun image(index: Int) = "{\"type\":\"image\",\"attrs\":{\"src\":\"https://example.test/$index.png\"}}"
        val cellImageCount = imageCount - 1
        val firstCellCount = cellImageCount / 2
        val firstCell = (0 until firstCellCount).joinToString(",", transform = ::image)
        val secondCell = (firstCellCount until cellImageCount).joinToString(",", transform = ::image)
        return """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[$firstCell]},{"type":"table_cell","content":[$secondCell]}]}]},${image(imageCount - 1)}]}"""
    }

    private fun deferredImageTableSource(): String =
        """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[390]},"content":[{"type":"image","attrs":{"src":"https://example.test/deferred-table.png"}}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[390]},"content":[{"type":"paragraph","content":[{"type":"text","text":"table footer"}]}]}]}]}]}"""

    private fun cellReuseSource(beforeTable: String, cellText: String = "linked cell"): String =
        """{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"$beforeTable"}]},
            {"type":"table","content":[{"type":"table_row","content":[
                {"type":"table_cell","attrs":{"colwidth":[300]},"content":[
                    {"type":"paragraph","content":[{"type":"text","text":"$cellText","marks":[{"type":"link","attrs":{"href":"https://cell.example/link"}}]}]},
                    {"type":"card"},
                    {"type":"image","attrs":{"src":"https://example.test/reuse.png","width":20,"height":10}},
                    {"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"nested rich cell"}]}]}]}]}
                ]},
                {"type":"table_cell","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"sibling rich cell"}]}]}
            ]}]},
            {"type":"paragraph","content":[{"type":"text","text":"after table"}]}
        ]}""".trimIndent()

    private fun tableCachePressureSource(): String {
        val text = "rich table content ".repeat(4_500)
        val cell = """{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"$text"}]}]}"""
        return """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[$cell,$cell]},{"type":"table_row","content":[$cell,$cell]}]}]}"""
    }

    private fun nestedHeaderImageSource(
        imageSource: String = "https://example.test/nested.png",
        secondColumnWidth: Int = 300,
        nestedSecondColumnWidth: Int? = null,
        declaredImageSize: Boolean = true,
        laterOuterRow: Boolean = false,
        beforeText: String = "before"
    ): String {
        val nestedSecond = nestedSecondColumnWidth?.let {
            ",{\"type\":\"table_cell\",\"attrs\":{\"colwidth\":[$it]},\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"nested body\"}]}]}"
        } ?: ""
        val image = if (declaredImageSize) {
            """{"type":"image","attrs":{"src":"$imageSource","width":20,"height":20}}"""
        } else {
            """{"type":"image","attrs":{"src":"$imageSource"}}"""
        }
        val laterRow = if (laterOuterRow) {
            """,{"type":"table_row","content":[
                {"type":"table_cell","attrs":{"colwidth":[300]},"content":[{"type":"paragraph","content":[{"type":"text","text":"later stable left"}]}]},
                {"type":"table_cell","attrs":{"colwidth":[$secondColumnWidth]},"content":[{"type":"paragraph","content":[{"type":"text","text":"later stable right"}]}]}
            ]}"""
        } else ""
        return """{"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"$beforeText"}]},
            {"type":"table","content":[{"type":"table_row","content":[
                {"type":"table_header","attrs":{"colwidth":[300]},"content":[
                    {"type":"blockquote","content":[
                        {"type":"paragraph","content":[{"type":"text","text":"quoted"}]},
                        {"type":"table","content":[{"type":"table_row","content":[
                            {"type":"table_header","content":[
                                $image
                            ]}$nestedSecond
                        ]}]}
                    ]}
                ]},
                {"type":"table_cell","attrs":{"colwidth":[$secondColumnWidth]},"content":[
                    {"type":"paragraph","content":[{"type":"text","text":"body"}]}
                ]}
            ]}$laterRow]},
            {"type":"paragraph","content":[{"type":"text","text":"after"}]}
        ]}""".trimIndent()
    }

    private fun identicalLinkCellsSource(): String =
        """{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[
            {"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same link","marks":[{"type":"link","attrs":{"href":"https://same.example/link"}}]}]}]},
            {"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same link","marks":[{"type":"link","attrs":{"href":"https://same.example/link"}}]}]}]}
        ]}]}]}"""

    private fun viewerAtomTheme(revision: String, docPos: Long, height: Int, width: Int): String =
        """{"viewerAtoms":{"generation":"atom-geometry","revision":"$revision","nodeTypes":["card"],"estimatedHeights":{"card":36},"measurements":{"$docPos":{"width":$width,"height":$height}}}}"""

    private fun nestedTablesSource(depth: Int): String {
        var node = """{"type":"paragraph","content":[{"type":"text","text":"deep"}]}"""
        repeat(depth) {
            node = """{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[$node]}]}]}"""
        }
        return """{"type":"doc","content":[$node]}"""
    }

    private class CountingAdmissionLayoutEngine : AndroidProseLayoutEngine {
        var preparationCount = 0

        override fun prepare(
            document: com.apollohg.editor.viewer.ViewerDocument,
            key: ProseLayoutKey,
            theme: PreparedProseTheme,
            widthPx: Int,
            density: Float,
            collapsesWhenEmpty: Boolean
        ): PreparedProseLayout {
            preparationCount += 1
            return PreparedProseLayout(key, widthPx, 0, emptyList(), retainedBytes = 0)
        }
    }

    private companion object {
        const val CONFIG = """{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"codeBlock","content":"inline*","group":"block","role":"textBlock","attrs":{"language":{"default":null}}},{"name":"text","content":"","group":"inline","role":"text"},{"name":"blockquote","content":"block+","group":"block","role":"block"},{"name":"bulletList","content":"listItem+","group":"block","role":"list"},{"name":"listItem","content":"block+","role":"listItem","attrs":{"checked":{"default":false}}},{"name":"image","content":"","group":"block","role":"block","isVoid":true,"attrs":{"src":{"default":""},"width":{"default":null},"height":{"default":null}}},{"name":"card","content":"","group":"block","role":"block","isVoid":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[{"name":"bold"}]},"initialization":{"type":"localEmpty"}}"""
    }
}
