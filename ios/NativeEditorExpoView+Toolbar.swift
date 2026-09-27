import ExpoModulesCore
import UIKit

extension NativeEditorExpoView {
    func updateAccessoryToolbarVisibility() {
        guard prepareForInputAccessoryMutationOrRetry(.updateAccessoryToolbarVisibility) else { return }
        refreshSystemAssistantToolbarIfNeeded()
        let nextAccessoryView: UIView?
        if showsToolbar &&
            toolbarPlacement == "keyboard" &&
            richTextView.textView.isEditable &&
            !shouldUseSystemAssistantToolbar {
            nextAccessoryView = accessoryToolbar
        } else if richTextView.textView.isEditable && !shouldUseSystemAssistantToolbar {
            nextAccessoryView = accessoryPlaceholder
        } else {
            nextAccessoryView = nil
        }
        for input in richTextView.textInputs where input.inputAccessoryView !== nextAccessoryView {
            input.inputAccessoryView = nextAccessoryView
            if input.isFirstResponder {
                input.reloadInputViews()
            }
        }
        markAccessoryMutationSucceeded(.updateAccessoryToolbarVisibility)
    }

    func refreshSystemAssistantToolbarIfNeeded() {
        guard #available(iOS 26.0, *) else { return }

        for input in richTextView.textInputs {
            let assistantItem = input.inputAssistantItem
            assistantItem.allowsHidingShortcuts = false
            assistantItem.leadingBarButtonGroups = []
            assistantItem.trailingBarButtonGroups = []
        }
    }

    private func handleListToggle(_ listType: String, in input: EditorTextView) {
        let isActive = toolbarState.nodes[listType] == true
        input.performToolbarToggleList(listType, isActive: isActive)
    }

    func handleToolbarItemPress(_ item: NativeToolbarItem) {
        let originatingEditorId = richTextView.editorId
        let input = richTextView.activeTextInput
        switch item.type {
        case .mark:
            guard let mark = item.mark else { return }
            input.performToolbarToggleMark(mark)
        case .heading:
            guard let level = item.headingLevel else { return }
            input.performToolbarToggleHeading(level)
        case .blockquote:
            input.performToolbarToggleBlockquote()
        case .list:
            guard let listType = item.listType?.rawValue else { return }
            handleListToggle(listType, in: input)
        case .command:
            switch item.command {
            case .indentList:
                input.performToolbarIndentListItem()
            case .outdentList:
                input.performToolbarOutdentListItem()
            case .undo:
                richTextView.textView.performToolbarUndo()
            case .redo:
                richTextView.textView.performToolbarRedo()
            case .none:
                break
            }
        case .node:
            guard let nodeType = item.nodeType else { return }
            input.performToolbarInsertNode(nodeType)
        case .action:
            guard let key = item.key else { return }
            guard let event = Self.editorScopedEventPayload(
                ["key": key],
                originatingEditorId: originatingEditorId
            ) else { return }
            onToolbarAction(event)
        case .group:
            break
        case .separator:
            break
        }
    }

}
