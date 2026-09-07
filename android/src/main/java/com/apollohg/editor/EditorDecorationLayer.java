package com.apollohg.editor;

import android.content.Context;
import android.widget.FrameLayout;
import com.facebook.react.uimanager.PointerEvents;
import com.facebook.react.uimanager.ReactPointerEventsView;

// Java supports both RN's original getter and its Kotlin property interface.
final class EditorDecorationLayer extends FrameLayout implements ReactPointerEventsView {
    EditorDecorationLayer(Context context) {
        super(context);
    }

    @Override
    public PointerEvents getPointerEvents() {
        return PointerEvents.NONE;
    }
}
