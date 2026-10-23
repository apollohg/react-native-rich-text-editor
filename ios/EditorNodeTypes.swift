import Foundation

enum EditorNodeTypes {
    static let headingTypes: Set<String> = ["heading", "h1", "h2", "h3", "h4", "h5", "h6"]

    static func listItemType(for listType: String) -> String {
        switch listType {
        case "bullet_list", "ordered_list":
            return "list_item"
        case "task_list":
            return "task_item"
        case "taskList":
            return "taskItem"
        default:
            return "listItem"
        }
    }

    static func isHardBreak(_ nodeType: String?) -> Bool {
        nodeType == "hardBreak" || nodeType == "hard_break"
    }

    static func isHorizontalRule(_ nodeType: String?) -> Bool {
        nodeType == "horizontalRule" || nodeType == "horizontal_rule"
    }

    static func isListItem(_ nodeType: String) -> Bool {
        nodeType == "listItem" || nodeType == "list_item"
            || nodeType == "taskItem" || nodeType == "task_item"
    }

    static func isListContainer(_ nodeType: String) -> Bool {
        nodeType == "bulletList" || nodeType == "bullet_list"
            || nodeType == "orderedList" || nodeType == "ordered_list"
            || nodeType == "taskList" || nodeType == "task_list"
    }

    static func preferredHardBreak(in insertableNodes: Set<String>) -> String {
        insertableNodes.contains("hard_break") ? "hard_break" : "hardBreak"
    }
}

enum EditorTextAlignment: String {
    case left
    case center
    case right

    static func isValid(_ value: Any?) -> Bool {
        guard let value = value as? String else { return false }
        return Self(rawValue: value) != nil
    }
}

enum EditorNodeSelection {
    static func scalarRange(_ selection: [String: Any]) -> (start: UInt32, end: UInt32)? {
        guard selection["type"] as? String == "node",
              let start = v2ExactUInt32(selection["posScalar"] as? NSNumber) else { return nil }
        if let value = selection["endScalar"] {
            guard let end = v2ExactUInt32(value as? NSNumber), end >= start else { return nil }
            return (start, end)
        }
        guard start < UInt32.max else { return nil }
        return (start, start + 1)
    }
}
