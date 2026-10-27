package com.apollohg.editor

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PorterDuff
import android.graphics.PorterDuffColorFilter
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.Drawable
import android.net.Uri
import androidx.appcompat.widget.AppCompatButton
import com.facebook.common.executors.UiThreadImmediateExecutorService
import com.facebook.common.references.CloseableReference
import com.facebook.datasource.BaseDataSubscriber
import com.facebook.datasource.DataSource
import com.facebook.drawee.backends.pipeline.Fresco
import com.facebook.imagepipeline.common.ResizeOptions
import com.facebook.imagepipeline.image.CloseableBitmap
import com.facebook.imagepipeline.image.CloseableImage
import com.facebook.imagepipeline.request.ImageRequest
import com.facebook.imagepipeline.request.ImageRequestBuilder
import com.facebook.react.views.imagehelper.ImageSource
import kotlin.math.roundToInt

internal class ToolbarImageRequest(
    context: Context,
    source: NativeToolbarImageSource,
    sizePx: Int,
    fetch: (ImageRequest, Context) -> DataSource<CloseableReference<CloseableImage>> =
        { request, caller -> Fresco.getImagePipeline().fetchDecodedImage(request, caller) },
    private val onImage: (Drawable?) -> Unit
) {
    private var image: CloseableReference<CloseableImage>? = null
    private var closed = false
    private val request: DataSource<CloseableReference<CloseableImage>>

    init {
        val uri = if (source.uri.startsWith("/")) {
            Uri.fromFile(java.io.File(source.uri))
        } else {
            ImageSource(context, source.uri).uri
        }
        val imageRequest = ImageRequestBuilder.newBuilderWithSource(uri)
            .setResizeOptions(ResizeOptions(sizePx.coerceAtLeast(1), sizePx.coerceAtLeast(1)))
            .build()
        request = fetch(imageRequest, context)
        request.subscribe(
            object : BaseDataSubscriber<CloseableReference<CloseableImage>>() {
                override fun onNewResultImpl(
                    dataSource: DataSource<CloseableReference<CloseableImage>>
                ) {
                    if (!dataSource.isFinished || closed) return
                    image = dataSource.result
                    val bitmap = image?.get() as? CloseableBitmap
                    onImage(
                        bitmap?.let {
                            BitmapDrawable(context.resources, it.underlyingBitmap)
                        }
                    )
                }

                override fun onFailureImpl(
                    dataSource: DataSource<CloseableReference<CloseableImage>>
                ) {
                    if (!closed) onImage(null)
                }
            },
            UiThreadImmediateExecutorService.getInstance()
        )
    }

    fun close() {
        closed = true
        request.close()
        CloseableReference.closeSafely(image)
        image = null
    }
}

internal class ToolbarImageButton(context: Context) : AppCompatButton(context) {
    companion object {
        const val DISCLOSURE_SIZE_DP = 8f
        const val DISCLOSURE_GAP_DP = 4f
        const val DISCLOSURE_SPACE_DP = DISCLOSURE_SIZE_DP + DISCLOSURE_GAP_DP
        private const val DISCLOSURE_STROKE_DP = 1.5f
    }

    var disclosure = ToolbarGroupDisclosure.NONE
    val showsDisclosure: Boolean
        get() = disclosure != ToolbarGroupDisclosure.NONE
    private val disclosurePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val disclosurePath = Path()
    var imageSource: NativeToolbarImageSource? = null
    var imageTintable = true
    var imageDrawable: Drawable? = null
        set(value) {
            field = value
            invalidate()
        }
    private var imageRequest: ToolbarImageRequest? = null
    private var requestGeneration = 0
    private var requestedSource: NativeToolbarImageSource? = null
    private var requestedSize = 0
    private var imageColorFilter: PorterDuffColorFilter? = null
    private var imageTintColor: Int? = null

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        refreshImageRequest()
    }

    fun refreshImageRequest() {
        val size = textSize.roundToInt()
        if (requestedSource == imageSource && requestedSize == size) return
        requestGeneration++
        imageRequest?.close()
        imageRequest = null
        imageDrawable = null
        requestedSource = null
        if (!isAttachedToWindow) return
        requestedSize = size
        requestedSource = imageSource
        val source = imageSource ?: return
        val generation = requestGeneration
        imageRequest = ToolbarImageRequest(context, source, size) { drawable ->
            if (generation == requestGeneration) imageDrawable = drawable
        }
    }

    override fun onDetachedFromWindow() {
        requestGeneration++
        imageDrawable = null
        imageRequest?.close()
        imageRequest = null
        requestedSource = null
        requestedSize = 0
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val density = resources.displayMetrics.density
        val space = if (showsDisclosure) DISCLOSURE_SPACE_DP * density else 0f
        val direction = if (layoutDirection == LAYOUT_DIRECTION_RTL) -1f else 1f
        val saved = canvas.save()
        canvas.translate(-direction * space / 2, 0f)
        val drawable = imageDrawable
        val primaryWidth = if (drawable == null) {
            super.onDraw(canvas)
            paint.measureText(text.toString())
        } else {
            drawImageIcon(canvas, drawable)
        }
        canvas.restoreToCount(saved)
        if (!showsDisclosure) return
        val size = DISCLOSURE_SIZE_DP * density
        val center = width / 2f + direction * (primaryWidth / 2 + DISCLOSURE_GAP_DP * density / 2)
        val verticalDirection = if (disclosure == ToolbarGroupDisclosure.UP) -1f else 1f
        val top = height / 2f - verticalDirection * size / 4
        disclosurePaint.color = currentTextColor
        disclosurePaint.strokeWidth = DISCLOSURE_STROKE_DP * density
        disclosurePath.reset()
        disclosurePath.moveTo(center - size / 2, top)
        disclosurePath.lineTo(center, top + verticalDirection * size / 2)
        disclosurePath.lineTo(center + size / 2, top)
        canvas.drawPath(disclosurePath, disclosurePaint)
    }

    private fun drawImageIcon(canvas: Canvas, drawable: Drawable): Float {
        if (imageTintable) {
            val color = currentTextColor
            if (imageTintColor != color) {
                imageTintColor = color
                imageColorFilter = PorterDuffColorFilter(color, PorterDuff.Mode.SRC_IN)
            }
            drawable.colorFilter = imageColorFilter
        } else {
            drawable.clearColorFilter()
        }
        val edge = textSize.coerceAtMost(minOf(width, height).toFloat())
        val aspect = if (drawable.intrinsicWidth > 0 && drawable.intrinsicHeight > 0) {
            drawable.intrinsicWidth.toFloat() / drawable.intrinsicHeight
        } else {
            1f
        }
        val iconWidth = if (aspect > 1f) edge else edge * aspect
        val iconHeight = if (aspect > 1f) edge / aspect else edge
        val left = ((width - iconWidth) / 2).roundToInt()
        val top = ((height - iconHeight) / 2).roundToInt()
        drawable.setBounds(left, top, left + iconWidth.roundToInt(), top + iconHeight.roundToInt())
        drawable.draw(canvas)
        return iconWidth
    }
}
