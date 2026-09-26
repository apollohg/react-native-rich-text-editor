package com.apollohg.editor

import com.apollohg.editor.NativeEditorExpoView.Companion.EDITOR_UPDATE_EVENT_DEBOUNCE_MS
import com.apollohg.editor.NativeEditorExpoView.Companion.nanosToMicros
import com.apollohg.editor.NativeEditorExpoView.NativeCommitKey
import com.apollohg.editor.NativeEditorExpoView.PendingEditorUpdateEvent
import com.apollohg.editor.NativeEditorExpoView.PreflightUpdateEvent
import android.graphics.RectF
import android.view.View
import android.view.ViewTreeObserver
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.apollohg.editor.tables.TableSelectionGeometry
import com.apollohg.editor.tables.TableSelectionObstructions
import org.json.JSONObject

internal class TableSelectionGeometryPublisher(
    private val frameHost: View,
    private val resolve: () -> TableSelectionGeometry?,
    private val emit: (Map<String, Any>) -> Unit
) {
    private var published: TableSelectionGeometry? = null
    private var flushPosted = false
    private val scheduledFlush = Runnable {
        flushPosted = false
        flush()
    }
    private val layoutListener = ViewTreeObserver.OnGlobalLayoutListener { scheduleFlush() }
    private var observedTree: ViewTreeObserver? = null

    fun observeWindowLayout() {
        if (observedTree?.isAlive == true) return
        observedTree = frameHost.viewTreeObserver.also { it.addOnGlobalLayoutListener(layoutListener) }
    }

    fun stopObservingWindowLayout() {
        observedTree?.takeIf { it.isAlive }?.removeOnGlobalLayoutListener(layoutListener)
        observedTree = null
    }

    val hasScheduledFlushForTesting: Boolean get() = flushPosted

    fun scheduleFlush() {
        if (flushPosted) return
        flushPosted = true
        frameHost.postOnAnimation(scheduledFlush)
    }

    fun flush() {
        cancelScheduledFlush()
        val current = resolve()
        val previous = published
        if (previous != null && current?.editorId != previous.editorId) {
            published = null
            emit(mapOf("editorId" to previous.editorId))
        }
        if (current == null || current == published) return
        published = current
        emit(current.eventPayload())
    }

    fun cancelScheduledFlush() {
        if (flushPosted) frameHost.removeCallbacks(scheduledFlush)
        flushPosted = false
    }
}

internal fun NativeEditorExpoView.currentTableSelectionGeometry(): TableSelectionGeometry? {
    if (!isAttachedToNativeWindow || !richTextView.activeTextInput.hasFocus()) return null
    return richTextView.tableSelectionGeometry(tableSelectionObstructions())
        ?.takeIf { it.editorId == eventEditorId(richTextView.editorId) }
}

internal fun NativeEditorExpoView.tableSelectionObstructions(): TableSelectionObstructions {
    val window = rootView
    val density = resources.displayMetrics.density
    val insets = rootWindowInsetsForTesting ?: ViewCompat.getRootWindowInsets(this) ?: WindowInsetsCompat.CONSUMED
    val unsafe = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
    val keyboardHeight = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
    val width = window.width.toFloat()
    val height = window.height.toFloat()
    return TableSelectionObstructions(
        safeArea = RectF(
            unsafe.left / density, unsafe.top / density,
            (width - unsafe.right) / density, (height - unsafe.bottom) / density
        ),
        keyboard = if (keyboardHeight > 0) {
            RectF(0f, (height - keyboardHeight) / density, width / density, height / density)
        } else {
            null
        }
    )
}

internal fun NativeEditorExpoView.dispatchTableSelectionGeometry(payload: Map<String, Any>) {
    onTableSelectionGeometryForTesting?.invoke(payload) ?: onTableSelectionGeometry(payload)
}

internal fun NativeEditorExpoView.documentVersionFromUpdateJSON(updateJSON: String?): String? =
    try {
        if (updateJSON == null) {
            null
        } else {
            canonicalV2U64(JSONObject(updateJSON).opt("documentVersion") as? String)
        }
    } catch (_: Throwable) {
        null
    }

internal fun NativeEditorExpoView.noteDocumentVersionFromUpdateJSON(updateJSON: String?) {
    documentVersionFromUpdateJSON(updateJSON)?.let { version ->
        lastDocumentVersion = version
    }
}

internal fun NativeEditorExpoView.isSupersededEditorUpdate(updateJson: String): Boolean {
    val rendered = renderedDocumentRevision?.toULongOrNull() ?: return false
    val incoming = documentVersionFromUpdateJSON(updateJson)?.toULongOrNull() ?: return false
    return incoming < rendered
}

internal fun NativeEditorExpoView.preflightUpdateEventFromJSON(
    updateJSON: String?
): PreflightUpdateEvent? {
    val update = updateJSON ?: return null
    val documentRevision = documentVersionFromUpdateJSON(update) ?: return null
    return PreflightUpdateEvent(updateJSON = update, documentRevision = documentRevision)
}

internal fun NativeEditorExpoView.addPreflightUpdateToEvent(
    event: MutableMap<String, Any>,
    preflightUpdate: PreflightUpdateEvent?
) {
    preflightUpdate ?: return
    event["updateJson"] = preflightUpdate.updateJSON
    event["documentRevision"] = preflightUpdate.documentRevision
}

internal fun NativeEditorExpoView.emitAddonEvent(payload: Map<String, Any>) {
    onAddonEventForTesting?.invoke(payload) ?: onAddonEvent(payload)
}

internal fun NativeEditorExpoView.pendingEditorUpdateEventCountForTestingImpl(): Int =
    pendingEditorUpdateEvents.size

internal fun NativeEditorExpoView.schedulePendingEditorUpdateDispatch() {
    pendingEditorUpdateDispatchScheduled = true
    val generation = ++pendingEditorUpdateDispatchGeneration
    mainHandler.postDelayed({
        if (generation != pendingEditorUpdateDispatchGeneration) return@postDelayed
        pendingEditorUpdateDispatchScheduled = false
        drainPendingEditorUpdateEvents()
    }, EDITOR_UPDATE_EVENT_DEBOUNCE_MS)
}

internal fun NativeEditorExpoView.drainPendingEditorUpdateEvents() {
    if (pendingEditorUpdateEvents.isEmpty()) return
    val startedAt = System.nanoTime()
    var drainedCount = 0
    while (pendingEditorUpdateEvents.isNotEmpty()) {
        val event = pendingEditorUpdateEvents.removeFirst()
        pendingEditorUpdateKeys.remove(NativeCommitKey(event.editorId, event.documentRevision))
        if (event.editorId != eventEditorId(richTextView.editorId)) {
            richTextView.editorEditText.recordImeTraceForTesting(
                "nativeViewEditorUpdateSkipped",
                "reason=staleEditor queuedEditor=${event.editorId} currentEditor=${eventEditorId(
                    richTextView.editorId
                )}"
            )
            continue
        }
        val isCurrentRevision = event.documentRevision == renderedDocumentRevision
        dispatchEditorUpdate(event, emitToJS = true, applyViewState = isCurrentRevision)
        drainedCount += 1
    }
    richTextView.editorEditText.recordImeTraceForTesting(
        "nativeViewEditorUpdateDrained",
        "count=$drainedCount totalUs=${nanosToMicros(System.nanoTime() - startedAt)}"
    )
}

internal fun NativeEditorExpoView.dispatchEditorUpdate(
    event: PendingEditorUpdateEvent,
    emitToJS: Boolean,
    applyViewState: Boolean = true
) {
    val updateJSON = event.viewUpdateJSON
    val startedAt = System.nanoTime()
    if (applyViewState) noteDocumentVersionFromUpdateJSON(updateJSON)
    val noteNanos = System.nanoTime() - startedAt
    val toolbarStartedAt = System.nanoTime()
    if (applyViewState) {
        NativeToolbarState.fromUpdateJson(updateJSON)?.let { state ->
            toolbarState = state
            keyboardToolbarView.applyState(state)
        }
    }
    val toolbarNanos = System.nanoTime() - toolbarStartedAt
    val mentionStartedAt = System.nanoTime()
    if (applyViewState) refreshMentionQuery()
    val mentionNanos = System.nanoTime() - mentionStartedAt
    val retryStartedAt = System.nanoTime()
    if (applyViewState) {
        clearPendingNativeActionRetryIfScopeChanged()
        schedulePendingPreflightWake()
        richTextView.refreshRemoteSelections()
    }
    val retryNanos = System.nanoTime() - retryStartedAt
    if (applyViewState && heightBehavior == EditorHeightBehavior.AUTO_GROW) {
        post {
            requestLayout()
            emitContentHeightIfNeeded(force = false)
        }
    }
    val emitStartedAt = System.nanoTime()
    if (emitToJS) {
        val payload = mapOf<String, Any>(
            "updateJson" to event.atomicUpdateJSON,
            "editorId" to event.editorId,
            "documentRevision" to event.documentRevision
        )
        onEditorUpdateForTesting?.invoke(payload) ?: onEditorUpdate(payload)
    }
    val totalNanos = System.nanoTime() - startedAt
    richTextView.editorEditText.recordImeTraceForTesting(
        "nativeViewEditorUpdateDispatch",
        "emitToJS=$emitToJS jsonLength=${updateJSON.length} noteUs=${nanosToMicros(
            noteNanos
        )} toolbarUs=${nanosToMicros(
            toolbarNanos
        )} mentionUs=${nanosToMicros(
            mentionNanos
        )} retryUs=${nanosToMicros(
            retryNanos
        )} emitUs=${nanosToMicros(
            System.nanoTime() - emitStartedAt
        )} totalUs=${nanosToMicros(totalNanos)}"
    )
}
