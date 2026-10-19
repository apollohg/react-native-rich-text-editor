package com.apollohg.editor

import android.os.Parcel
import android.text.Annotation
import androidx.annotation.Keep

internal class RenderMetadataAnnotation(key: String, initialValue: String) :
    Annotation(key, initialValue) {
    private var renderedValue = initialValue

    override fun getValue(): String = renderedValue

    fun updateValue(value: String) {
        renderedValue = value
    }

    override fun writeToParcel(destination: Parcel, flags: Int) {
        destination.writeString(key)
        destination.writeString(renderedValue)
    }

    // TextUtils invokes this framework hook when parceling spans.
    @Keep
    fun writeToParcelInternal(destination: Parcel, flags: Int) = writeToParcel(destination, flags)
}
