package com.apollohg.editor

import android.content.Context
import android.graphics.Canvas
import android.graphics.PorterDuff
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
    var imageSource: NativeToolbarImageSource? = null
    var imageTintable = true
    var imageDrawable: Drawable? = null
        set(value) {
            field = value
            invalidate()
        }
    private var imageRequest: ToolbarImageRequest? = null
    private var requestGeneration = 0

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        val source = imageSource ?: return
        val generation = ++requestGeneration
        imageRequest = ToolbarImageRequest(context, source, textSize.roundToInt()) { drawable ->
            if (generation == requestGeneration) imageDrawable = drawable
        }
    }

    override fun onDetachedFromWindow() {
        requestGeneration++
        imageDrawable = null
        imageRequest?.close()
        imageRequest = null
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val drawable = imageDrawable
        if (drawable == null) {
            super.onDraw(canvas)
            return
        }
        if (imageTintable) {
            drawable.setColorFilter(currentTextColor, PorterDuff.Mode.SRC_IN)
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
    }
}
