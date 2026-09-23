package com.apollohg.editor.tables

import android.view.inputmethod.EditorInfo
import com.apollohg.editor.EditorEditText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
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
class EditorTableInputTest {
    private fun input(): EditorEditText = EditorEditText(RuntimeEnvironment.getApplication())

    private fun binding(cellSourcePos: Long = 10) = TableCellPositionMap.Binding(
        cellSourcePos = cellSourcePos,
        revision = "4",
        epoch = "9"
    )

    private fun map(
        binding: TableCellPositionMap.Binding = binding(),
        segments: List<TableCellPositionMap.Segment> = listOf(
            TableCellPositionMap.Segment(0, 6, 40)
        )
    ) = TableCellPositionMap(binding, segments)

    @Test
    fun `position map converts local UTF-16 offsets around surrogate pairs`() {
        val positionMap = map()

        assertEquals(42, positionMap.globalScalarForLocalUtf16(3, "a😀bc"))
        assertEquals(
            TableCellPositionMap.ScalarRange(41, 42),
            positionMap.globalScalarRangeForLocalUtf16(1, 3, "a😀bc")
        )
    }

    @Test
    fun `position map excludes the final caret boundary unless projection covers it`() {
        val positionMap = map(segments = listOf(TableCellPositionMap.Segment(0, 5, 40)))

        assertEquals(44, positionMap.globalScalarForLocalScalar(4))
        assertEquals(4, positionMap.localScalarForGlobalScalar(44))
        assertNull(positionMap.globalScalarForLocalScalar(5))
        assertNull(positionMap.localScalarForGlobalScalar(45))
        assertNull(positionMap.globalScalarRange(4, 5))

        val projected = map(segments = listOf(TableCellPositionMap.Segment(0, 6, 40)))
        assertEquals(45, projected.globalScalarForLocalScalar(5))
        assertEquals(5, projected.localScalarForGlobalScalar(45))
        assertEquals(
            TableCellPositionMap.ScalarRange(44, 45),
            projected.globalScalarRangeForLocalUtf16(4, 5, "abcde")
        )
    }

    @Test
    fun `position map rejects local holes and global discontinuities`() {
        val continuous = map(segments = listOf(
            TableCellPositionMap.Segment(4, 8, 14),
            TableCellPositionMap.Segment(0, 4, 10)
        ))
        assertEquals(TableCellPositionMap.ScalarRange(11, 17), continuous.globalScalarRange(1, 7))

        val localHole = map(segments = listOf(
            TableCellPositionMap.Segment(0, 3, 10),
            TableCellPositionMap.Segment(4, 8, 14)
        ))
        val globalJump = map(segments = listOf(
            TableCellPositionMap.Segment(0, 4, 10),
            TableCellPositionMap.Segment(4, 8, 20)
        ))

        assertNull(localHole.globalScalarRange(0, 7))
        assertNull(globalJump.globalScalarRange(0, 7))
        assertNull(localHole.globalScalarForLocalScalar(3))
        assertNull(localHole.localScalarForGlobalScalar(13))
        assertEquals(14, localHole.globalScalarForLocalScalar(4))
        assertEquals(4, localHole.localScalarForGlobalScalar(14))
    }

    @Test
    fun `position map rejects ambiguous inverse coordinates and stale versions`() {
        val ambiguous = map(segments = listOf(
            TableCellPositionMap.Segment(0, 3, 10),
            TableCellPositionMap.Segment(3, 6, 11)
        ))

        assertNull(ambiguous.localScalarForGlobalScalar(11))
        assertEquals(0, ambiguous.localScalarForGlobalScalar(10))
        assertNull(ambiguous.globalScalarForLocalScalar(0, currentRevision = "5"))
        assertNull(ambiguous.globalScalarForLocalScalar(0, currentEpoch = "10"))
        assertNull(ambiguous.localScalarForGlobalScalar(10, currentRevision = "5"))
        assertNull(ambiguous.localScalarForGlobalScalar(10, currentEpoch = "10"))
    }

    @Test
    fun `coordinator accepts the last representable global caret`() {
        val target = binding()
        val maximalCaret = map(target, listOf(
            TableCellPositionMap.Segment(0, 1, Int.MAX_VALUE)
        ))
        val coordinator = EditorTableInputCoordinator(input())

        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(target), maximalCaret, "4", "9"))
        assertEquals(Int.MAX_VALUE, maximalCaret.globalScalarForLocalScalar(0))
        assertEquals(0, maximalCaret.localScalarForGlobalScalar(Int.MAX_VALUE))
        assertEquals(
            TableCellPositionMap.ScalarRange(Int.MAX_VALUE, Int.MAX_VALUE),
            maximalCaret.globalScalarRange(0, 0)
        )
        assertNull(maximalCaret.globalScalarForLocalScalar(1))
    }

    @Test
    fun `coordinator reuses its injected input through bound and composing phases`() {
        val input = input()
        val coordinator = EditorTableInputCoordinator(input)
        val first = binding(10)
        val second = binding(20)

        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(first), map(first), "4", "9"))
        assertTrue(coordinator.beginComposition())
        assertEquals(TableInputPhase.Composing(10, "4", "9"), coordinator.phase)
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(second), map(second), "4", "9"))
        assertEquals(TableInputPhase.Composing(10, "4", "9"), coordinator.phase)
        assertTrue(coordinator.invalidateBinding())
        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(second), map(second), "4", "9"))

        assertSame(input, coordinator.cellInput)
        assertEquals(1, coordinator.inputInstanceCountForTesting)
        assertEquals(TableInputPhase.Bound(20, "4", "9"), coordinator.phase)
    }

    @Test
    fun `coordinator binds valid segmented maps while cross gap ranges remain rejected`() {
        val input = input()
        val coordinator = EditorTableInputCoordinator(input)
        val target = binding(20)
        val segmented = map(target, listOf(
            TableCellPositionMap.Segment(0, 2, 50),
            TableCellPositionMap.Segment(3, 5, 70)
        ))

        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(target), segmented, "4", "9"))
        assertNull(segmented.globalScalarRange(0, 4))
        assertSame(input, coordinator.cellInput)
        assertEquals(TableInputPhase.Bound(20, "4", "9"), coordinator.phase)
    }

    @Test
    fun `coordinator retires injected input before binding and invalidating`() {
        val input = input()
        val coordinator = EditorTableInputCoordinator(input)
        val target = binding()
        input.composingText = "stale"
        val staleConnection = input.onCreateInputConnection(EditorInfo())
        assertNotNull(staleConnection)
        assertSame(staleConnection, input.activeInputConnection)
        val generationBeforeBind = input.inputConnectionGenerationForTesting()

        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(target), map(target), "4", "9"))
        assertNull(input.composingTextForEditor())
        assertNull(input.activeInputConnection)
        val generationAfterBind = input.inputConnectionGenerationForTesting()
        assertTrue(generationAfterBind > generationBeforeBind)

        input.composingText = "stale again"
        val secondConnection = input.onCreateInputConnection(EditorInfo())
        assertNotNull(secondConnection)
        assertSame(secondConnection, input.activeInputConnection)
        assertTrue(coordinator.invalidateBinding())
        assertNull(input.composingTextForEditor())
        assertNull(input.activeInputConnection)
        assertTrue(input.inputConnectionGenerationForTesting() > generationAfterBind)
    }

    @Test
    fun `coordinator rejects unsafe and stale targets without changing its binding`() {
        val input = input()
        val coordinator = EditorTableInputCoordinator(input)
        val valid = binding(10)
        assertTrue(coordinator.bind(EditorTableInputCoordinator.Target(valid), map(valid), "4", "9"))
        input.composingText = "retained"
        val retainedConnection = input.onCreateInputConnection(EditorInfo())
        assertNotNull(retainedConnection)
        val retainedGeneration = input.inputConnectionGenerationForTesting()

        listOf(
            EditorTableInputCoordinator.Target(binding(20), isSynthetic = true),
            EditorTableInputCoordinator.Target(binding(20), isNestedTarget = true),
            EditorTableInputCoordinator.Target(binding(20), hasExcludedContent = true)
        ).forEach { target ->
            assertFalse(coordinator.bind(target, map(target.binding), "4", "9"))
            assertEquals(TableInputPhase.Bound(10, "4", "9"), coordinator.phase)
            assertSame(input, coordinator.cellInput)
        }

        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), map(binding(20)), "5", "9"))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), map(binding(20)), "4", "10"))
        val mismatched = map(binding(30))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), mismatched, "4", "9"))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), map(binding(20), emptyList()), "4", "9"))
        val emptySegment = map(binding(20), listOf(
            TableCellPositionMap.Segment(0, 0, 50)
        ))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), emptySegment, "4", "9"))
        val negativeSegment = map(binding(20), listOf(
            TableCellPositionMap.Segment(-1, 2, 50)
        ))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), negativeSegment, "4", "9"))
        val negativeGlobal = map(binding(20), listOf(
            TableCellPositionMap.Segment(0, 2, -1)
        ))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), negativeGlobal, "4", "9"))
        val overflowing = map(binding(20), listOf(
            TableCellPositionMap.Segment(0, 2, Int.MAX_VALUE)
        ))
        assertFalse(coordinator.bind(EditorTableInputCoordinator.Target(binding(20)), overflowing, "4", "9"))
        assertEquals(TableInputPhase.Bound(10, "4", "9"), coordinator.phase)
        assertEquals(40, coordinator.positionMap?.globalScalarForLocalScalar(0))
        assertEquals("retained", input.composingTextForEditor())
        assertSame(retainedConnection, input.activeInputConnection)
        assertEquals(retainedGeneration, input.inputConnectionGenerationForTesting())
    }
}
