package com.apollohg.editor

import android.os.Handler
import android.os.Looper
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

private const val DESTROY_INVALIDATION_AWAIT_TIMEOUT_MS = 250L

internal enum class NativeEditorDestroyReservationResult {
    RESERVED,
    ALREADY_IN_PROGRESS,
    UNAVAILABLE
}

private class WeakNativeEditorExpoView private constructor(
    val view: WeakReference<NativeEditorExpoView?>
) {
    constructor(view: NativeEditorExpoView) : this(WeakReference(view))

    companion object {
        fun cleared(): WeakNativeEditorExpoView =
            WeakNativeEditorExpoView(WeakReference<NativeEditorExpoView?>(null))
    }
}

internal object NativeEditorViewRegistry {
    private val liveEditorIds = mutableSetOf<Long>()
    private val viewsByEditorId = mutableMapOf<Long, MutableList<WeakNativeEditorExpoView>>()
    private val inputViewsByEditorId =
        mutableMapOf<Long, MutableList<WeakReference<EditorEditText>>>()
    private val detachedEditorOwnersByEditorId = mutableMapOf<Long, WeakNativeEditorExpoView>()
    private val destroyingEditorIds = mutableSetOf<Long>()
    private val destroyReservationWasLive = mutableMapOf<Long, Boolean>()
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    internal var onFinalizeDestroyForTesting: ((Long) -> Unit)? = null

    @Synchronized
    fun markEditorCreated(editorId: Long) {
        if (editorId == 0L) return
        liveEditorIds.add(editorId)
        destroyingEditorIds.remove(editorId)
        destroyReservationWasLive.remove(editorId)
    }

    @Synchronized
    fun register(editorId: Long, view: NativeEditorExpoView): Boolean {
        if (editorId == 0L) return false
        if (destroyingEditorIds.contains(editorId)) return false
        val isKnownDetachedOwner = detachedEditorOwnersByEditorId[editorId]?.view?.get() === view
        if (
            !liveEditorIds.contains(editorId) &&
            !isKnownDetachedOwner &&
            !rustEditorExists(editorId)
        ) {
            return false
        }
        val views = viewsByEditorId.getOrPut(editorId) { mutableListOf() }
        views.removeAll { it.view.get() == null || it.view.get() === view }
        views += WeakNativeEditorExpoView(view)
        detachedEditorOwnersByEditorId.remove(editorId)
        return true
    }

    @Synchronized
    fun unregister(
        editorId: Long,
        view: NativeEditorExpoView,
        blockCommandsUntilRegistered: Boolean = false
    ) {
        if (editorId == 0L) return
        val registeredViews = viewsByEditorId[editorId]
        val wasRegistered = registeredViews?.any { it.view.get() === view } == true
        registeredViews?.removeAll { it.view.get() == null || it.view.get() === view }
        if (registeredViews?.isEmpty() == true) viewsByEditorId.remove(editorId)
        if (blockCommandsUntilRegistered) {
            detachedEditorOwnersByEditorId[editorId] = WeakNativeEditorExpoView(view)
        } else {
            val detachedOwner = detachedEditorOwnersByEditorId[editorId]?.view?.get()
            if (wasRegistered || detachedOwner === view) {
                detachedEditorOwnersByEditorId.remove(editorId)
            }
        }
    }

    @Synchronized
    fun isDestroyed(editorId: Long): Boolean = destroyingEditorIds.contains(editorId)

    @Synchronized
    internal fun retainedDestroyedIdCountForTests(): Int = destroyingEditorIds.size

    @Synchronized
    internal fun forceRegisteredViewsClearedForTesting(editorId: Long) {
        viewsByEditorId[editorId] = mutableListOf(WeakNativeEditorExpoView.cleared())
    }

    @Synchronized
    internal fun boundViewReferenceCountForTests(editorId: Long): Int =
        viewsByEditorId[editorId]?.size ?: 0

    @Synchronized
    private fun liveViewsFor(editorId: Long): List<NativeEditorExpoView> =
        viewsByEditorId[editorId]?.mapNotNull { it.view.get() }.orEmpty()

    fun rebaseAfterRemoteCommit(handle: String) {
        val viewToken = EditorV2Registry.viewTokenForHandle(handle) ?: return
        liveViewsFor(viewToken).forEach { view ->
            if (view.markRemoteCommitRebaseScheduled(viewToken)) {
                mainHandler.post { view.applyRemoteCommitRefresh(viewToken) }
            }
        }
    }

    @Synchronized
    fun registerInputView(editorId: Long, view: EditorEditText) {
        if (editorId == 0L || destroyingEditorIds.contains(editorId)) return
        if (!liveEditorIds.contains(editorId) && !rustEditorExists(editorId)) return
        val views = inputViewsByEditorId.getOrPut(editorId) { mutableListOf() }
        views.removeAll { it.get() == null || it.get() === view }
        views += WeakReference(view)
    }

    @Synchronized
    fun acquireDestroyReservation(editorId: Long): NativeEditorDestroyReservationResult {
        if (editorId == 0L) return NativeEditorDestroyReservationResult.UNAVAILABLE
        if (destroyingEditorIds.contains(editorId)) {
            return NativeEditorDestroyReservationResult.ALREADY_IN_PROGRESS
        }
        if (!liveEditorIds.contains(editorId) && !rustEditorExists(editorId)) {
            return NativeEditorDestroyReservationResult.UNAVAILABLE
        }
        destroyingEditorIds.add(editorId)
        destroyReservationWasLive[editorId] = liveEditorIds.remove(editorId)
        return NativeEditorDestroyReservationResult.RESERVED
    }

    fun beginDestroy(editorId: Long): Boolean =
        acquireDestroyReservation(editorId) == NativeEditorDestroyReservationResult.RESERVED

    /** Undo an in-flight reservation after an FFI result that is retryable or malformed. */
    @Synchronized
    fun rollbackDestroy(editorId: Long) {
        if (!destroyingEditorIds.remove(editorId)) return
        if (destroyReservationWasLive.remove(editorId) == true) {
            liveEditorIds.add(editorId)
        }
    }

    fun invalidateDestroyedEditor(editorId: Long) {
        if (!beginDestroy(editorId)) return
        finalizeDestroy(editorId)
    }

    /**
     * Finalize a reserved destroy. Off-main callers may time out while the
     * main-thread invalidation is still queued, so completion is reported only
     * after the reservation itself has been released.
     */
    fun finalizeDestroy(editorId: Long, onCompleted: (() -> Unit)? = null) {
        val affectedViews = synchronized(this) {
            if (!destroyingEditorIds.contains(editorId)) return@synchronized null
            val views = listOfNotNull(
                *viewsByEditorId.remove(editorId)
                    .orEmpty()
                    .mapNotNull { it.view.get() }
                    .toTypedArray(),
                detachedEditorOwnersByEditorId.remove(editorId)?.view?.get()
            ).distinct()
            val inputViews = inputViewsByEditorId.remove(editorId)
                .orEmpty()
                .mapNotNull { it.get() }
                .distinct()
            views to inputViews
        }
        if (affectedViews == null) {
            onCompleted?.invoke()
            return
        }
        fun releaseReservation() {
            synchronized(this) {
                destroyingEditorIds.remove(editorId)
                destroyReservationWasLive.remove(editorId)
            }
            onCompleted?.invoke()
        }
        invokeDestroyTestingHook(onFinalizeDestroyForTesting, editorId)
        if (affectedViews.first.isEmpty() && affectedViews.second.isEmpty()) {
            releaseReservation()
            return
        }
        val invalidate = Runnable {
            try {
                affectedViews.first.forEach { view ->
                    view.handleEditorDestroyed(editorId)
                }
                affectedViews.second.forEach { view ->
                    view.handleEditorDestroyedFromRegistry(editorId)
                }
            } finally {
                releaseReservation()
            }
        }
        if (Looper.myLooper() == Looper.getMainLooper()) {
            invalidate.run()
        } else {
            val latch = CountDownLatch(1)
            val posted = mainHandler.post {
                try {
                    invalidate.run()
                } finally {
                    latch.countDown()
                }
            }
            if (!posted) {
                releaseReservation()
                return
            }
            try {
                latch.await(DESTROY_INVALIDATION_AWAIT_TIMEOUT_MS, TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                Thread.currentThread().interrupt()
            } catch (_: Throwable) {
                return
            }
        }
    }

    private fun rustEditorExists(viewToken: Long): Boolean =
        EditorV2Registry.adapterForViewToken(viewToken) != null

}
