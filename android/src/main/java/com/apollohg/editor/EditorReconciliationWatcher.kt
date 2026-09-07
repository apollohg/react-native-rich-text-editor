package com.apollohg.editor

import android.text.Editable
import android.text.TextWatcher
import android.util.Log
import com.apollohg.editor.EditorEditText.Companion.LOG_TAG

internal class EditorReconciliationWatcher(private val editor: EditorEditText) : TextWatcher {

    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {
    }

    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
    }

    override fun afterTextChanged(s: Editable?) {
        with(editor) {
            invalidateImeTextCoordinateMapperForEditor()
            if (isApplyingRustState) return
            if (!hasLiveEditor()) return

            val currentText = s?.toString() ?: ""
            if (currentText == lastAuthorizedText) return

            val mutation = nativeTextMutationFromAuthorizedDiff(currentText)
            if (mutation != null &&
                shouldAdoptNativeTextMutation(mutation, allowAfterBlur = true)
            ) {
                commitNativeTextMutation(mutation)
                return
            }

            reconciliationCount++
            Log.w(
                LOG_TAG,
                "reconciliation: EditText diverged from Rust state" +
                    " (count=$reconciliationCount," +
                    " editText=${currentText.length} chars," +
                    " authorized=${lastAuthorizedText.length} chars)"
            )

            val stateJSON = v2Driver?.currentStateJson() ?: return
            applyUpdateJSON(stateJSON)
        }
    }
}
