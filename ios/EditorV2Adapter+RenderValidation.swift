import Foundation

extension EditorV2Adapter {
    /// One view-facing update plus the document's scalar extent (the lenient
    /// `UInt32.max` doc→scalar mapping, used to clamp transient-IME
    /// positions the way the legacy engine did).
    struct EditorV2DerivedUpdate {
        let updateJSON: String
        let scalarLength: UInt32
    }

    static func uint32Field(_ object: [String: Any], _ key: String) -> UInt32? {
        v2ExactUInt32(object[key] as? NSNumber)
    }

    struct AtomicRenderSnapshot {
        let renderObject: [String: Any]
        let tableAttributes: [String: [String: Any]]
        let tableRecords: [String: [String: Any]]
        let atomicRenderJSON: String
        let viewUpdateJSON: String
        let documentRevision: UInt64
        let stateRevision: UInt64
        let scalarLength: UInt32
        let selection: (anchor: UInt32, head: UInt32)?
        let activeState: [String: Any]
        let historyState: (canUndo: Bool, canRedo: Bool)
        let documentIsEmpty: Bool
        let positionEpoch: UInt64?
    }

    static func exactBool(_ value: Any?) -> Bool? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) == CFBooleanGetTypeID()
        else {
            return nil
        }
        return number.boolValue
    }

    private static func finiteNumber(_ value: Any?) -> NSNumber? {
        guard let number = value as? NSNumber,
              CFGetTypeID(number) != CFBooleanGetTypeID(),
              number.doubleValue.isFinite
        else {
            return nil
        }
        return number
    }

    private static func hasOnlyKeys(_ object: [String: Any], _ allowed: Set<String>) -> Bool {
        Set(object.keys).isSubset(of: allowed)
    }

    private static func isValidJSONValue(_ value: Any) -> Bool {
        if value is NSNull || value is String || exactBool(value) != nil || finiteNumber(value) != nil {
            return true
        }
        if let array = value as? [Any] {
            return array.allSatisfy(isValidJSONValue)
        }
        if let object = value as? [String: Any] {
            return object.values.allSatisfy(isValidJSONValue)
        }
        return false
    }

    private static func isValidRenderMark(_ value: Any) -> Bool {
        if value is String { return true }
        guard let object = value as? [String: Any],
              object["type"] is String
        else {
            return false
        }
        return object.values.allSatisfy(isValidJSONValue)
    }

    private static func isValidListContext(_ value: Any) -> Bool {
        guard let object = value as? [String: Any],
              hasOnlyKeys(
                object,
                ["ordered", "index", "total", "start", "isFirst", "isLast", "kind", "checked"]
              ),
              exactBool(object["ordered"]) != nil,
              uint32Field(object, "index") != nil,
              uint32Field(object, "total") != nil,
              uint32Field(object, "start") != nil,
              exactBool(object["isFirst"]) != nil,
              exactBool(object["isLast"]) != nil
        else {
            return false
        }
        if let kind = object["kind"], !(kind is NSNull), !(kind is String) { return false }
        if let checked = object["checked"], !(checked is NSNull), exactBool(checked) == nil { return false }
        return true
    }

    private static func isValidMentionThemeSection(
        _ value: Any,
        stringKeys: Set<String>,
        extraKeys: Set<String>
    ) -> Bool {
        guard let object = value as? [String: Any] else { return false }
        guard hasOnlyKeys(object, stringKeys.union(mentionThemeNumberKeys).union(extraKeys)) else {
            return false
        }
        for key in stringKeys where object[key] != nil {
            guard object[key] is String else { return false }
        }
        for key in mentionThemeNumberKeys where object[key] != nil {
            guard finiteNumber(object[key]) != nil else { return false }
        }
        if let fontWeight = object["fontWeight"] {
            guard let fontWeight = fontWeight as? String,
                  mentionThemeFontWeights.contains(fontWeight)
            else {
                return false
            }
        }
        return true
    }

    private static func isValidMentionTheme(_ value: Any) -> Bool {
        guard let object = value as? [String: Any] else { return false }
        guard hasOnlyKeys(object, ["node", "suggestions"]) else { return false }

        if let node = object["node"] {
            guard isValidMentionThemeSection(
                node,
                stringKeys: mentionNodeStringKeys,
                extraKeys: ["fontWeight", "style"]
            ) else {
                return false
            }
        }
        if let style = (object["node"] as? [String: Any])?["style"], !isValidMentionStyle(style) { return false }
        guard let suggestions = object["suggestions"] else { return true }
        guard isValidMentionThemeSection(
            suggestions,
            stringKeys: mentionSuggestionsStringKeys,
            extraKeys: ["option"]
        ) else {
            return false
        }
        guard let option = (suggestions as? [String: Any])?["option"] else { return true }
        return isValidMentionThemeSection(
            option,
            stringKeys: mentionOptionStringKeys,
            extraKeys: ["fontWeight"]
        )
    }

    private static func isValidMentionStyle(_ value: Any) -> Bool {
        guard let style = value as? [String: Any] else { return false }
        let colors: Set<String> = ["color", "backgroundColor", "textDecorationColor", "borderColor", "borderTopColor", "borderRightColor", "borderBottomColor", "borderLeftColor"]
        let dimensions: Set<String> = ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft", "fontSize", "lineHeight", "letterSpacing", "borderWidth", "borderTopWidth", "borderRightWidth", "borderBottomWidth", "borderLeftWidth", "borderRadius", "borderTopLeftRadius", "borderTopRightRadius", "borderBottomLeftRadius", "borderBottomRightRadius"]
        let strings: Set<String> = ["fontFamily", "fontWeight", "fontStyle", "textDecorationLine", "textDecorationStyle", "borderStyle"]
        guard hasOnlyKeys(style, colors.union(dimensions).union(strings)) else { return false }
        for key in colors where style[key] != nil {
            guard let color = style[key] as? String, color.range(of: "^#[0-9a-fA-F]{8}$", options: .regularExpression) != nil else { return false }
        }
        for key in dimensions where style[key] != nil {
            guard let number = finiteNumber(style[key])?.doubleValue else { return false }
            if ["fontSize", "lineHeight"].contains(key), number <= 0 { return false }
            if key.hasPrefix("border") || key.hasPrefix("padding"), number < 0 { return false }
        }
        for key in strings where style[key] != nil {
            guard let value = style[key] as? String else { return false }
            switch key {
            case "fontFamily": if value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return false }
            case "fontWeight": if !mentionThemeFontWeights.contains(value) { return false }
            case "fontStyle": if !["normal", "italic"].contains(value) { return false }
            case "textDecorationLine": if !["none", "underline", "line-through", "underline line-through"].contains(value) { return false }
            case "textDecorationStyle": if !["solid", "double", "dotted", "dashed"].contains(value) { return false }
            case "borderStyle": if !["solid", "dotted", "dashed"].contains(value) { return false }
            default: return false
            }
        }
        return true
    }

    private static func isValidRenderElement(_ value: Any) -> Bool {
        guard let object = value as? [String: Any],
              let type = object["type"] as? String
        else {
            return false
        }
        switch type {
        case "textRun":
            guard Set(object.keys) == ["type", "text", "marks"],
                  object["text"] is String,
                  let marks = object["marks"] as? [Any]
            else {
                return false
            }
            return marks.allSatisfy(isValidRenderMark)
        case "blockStart":
            guard hasOnlyKeys(object, ["type", "nodeType", "language", "depth", "listContext"]),
                  object["nodeType"] is String,
                  uint32Field(object, "depth") != nil
            else {
                return false
            }
            if let language = object["language"], !(language is NSNull), !(language is String) { return false }
            return object["listContext"].map(isValidListContext) ?? true
        case "blockEnd":
            return Set(object.keys) == ["type"]
        case "voidInline":
            guard hasOnlyKeys(object, ["type", "nodeType", "docPos", "attrs"]),
                  object["nodeType"] is String,
                  uint32Field(object, "docPos") != nil
            else {
                return false
            }
            return object["attrs"].map { $0 is [String: Any] } ?? true
        case "voidBlock":
            guard hasOnlyKeys(object, ["type", "nodeType", "docPos", "attrs", "atomId"]),
                  object["nodeType"] is String,
                  uint32Field(object, "docPos") != nil,
                  object["atomId"].map({ $0 is String }) ?? true
            else {
                return false
            }
            return object["attrs"].map { $0 is [String: Any] } ?? true
        case "opaqueInlineAtom":
            guard hasOnlyKeys(
                      object,
                      ["type", "nodeType", "label", "docPos", "attrs", "mentionTheme"]
                  ),
                  object["nodeType"] is String,
                  object["label"] is String,
                  uint32Field(object, "docPos") != nil,
                  object["attrs"].map({ $0 is [String: Any] }) ?? true
            else {
                return false
            }
            return object["mentionTheme"].map(isValidMentionTheme) ?? true
        case "opaqueBlockAtom":
            guard hasOnlyKeys(object, ["type", "nodeType", "label", "docPos", "attrs"]),
                  object["nodeType"] is String,
                  object["label"] is String,
                  uint32Field(object, "docPos") != nil
            else {
                return false
            }
            return object["attrs"].map { $0 is [String: Any] } ?? true
        default:
            return false
        }
    }

    static func parseTableAttributes(_ value: Any?) -> [String: [String: Any]]? {
        guard let raw = (value ?? [String: String]()) as? [String: String] else { return nil }
        var pool: [String: [String: Any]] = [:]
        var unique = Set<String>()
        var bytes = 0
        var entries = 0
        for (key, json) in raw {
            entries += 1
            bytes += json.utf8.count
            guard key.range(of: "^[0-9a-f]{64}$", options: .regularExpression) != nil,
                  entries <= 7_000_000, unique.insert(json).inserted, bytes <= 192 * 1024 * 1024,
                  let data = json.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { return nil }
            var pending: [(Any, Int)] = [(object, 0)]
            var work = 0
            while let (item, depth) = pending.popLast() {
                work += 1
                if work > json.utf8.count || depth > 1024 { return nil }
                if let number = item as? NSNumber, !number.doubleValue.isFinite { return nil }
                if let object = item as? [String: Any] { pending.append(contentsOf: object.values.map { ($0, depth + 1) }) }
                else if let array = item as? [Any] { pending.append(contentsOf: array.map { ($0, depth + 1) }) }
            }
            pool[key] = object
        }
        return pool
    }

    static func validSemanticRenderElements(_ values: [Any], tableAttributes: [String: [String: Any]] = [:], tableRecords: [String: [String: Any]] = [:], requireCompletePool: Bool = true) -> Bool {
        var pending = values.map { (value: $0, depth: 0, start: UInt64(0), end: UInt64(UInt32.max)) }
        var nodes = 0
        var slots: UInt64 = 0
        var referenced = Set<String>()
        var referencedAttributes = Set<String>()
        func number(_ object: [String: Any], _ key: String) -> UInt64? {
            uint32Field(object, key).map(UInt64.init)
        }
        func attrs(_ value: Any?) -> Bool {
            guard let key = value as? String else { return false }
            guard tableAttributes[key] != nil else { return false }
            referencedAttributes.insert(key)
            return true
        }
        let failures: Set<String> = ["gridLimit", "workLimit", "allocation", "invalidStructure", "invalidAttributes"]
        let diagnostics: Set<String> = ["virtual-grid-limit", "empty-reference-surface", "unsupported-row-role", "unsupported-cell-role", "ambiguous-source-map", "unsupported-gap-default", "overlapping-reference-cells", "unmapped-reference-cell", "nonrectangular-reference-cell", "zero-span-after-reference-pass"]
        while let entry = pending.popLast() {
            nodes += 1
            guard nodes + pending.count <= 7_000_000, entry.depth <= 1024,
                  let element = entry.value as? [String: Any] else { return false }
            if element["type"] as? String != "table" {
                guard isValidRenderElement(element) else { return false }
                if element["docPos"] != nil {
                    guard let pos = number(element, "docPos"), pos >= entry.start, pos < entry.end else { return false }
                }
                continue
            }
            guard Set(element.keys) == ["type", "tableId"], let tableId = element["tableId"] as? String,
                  tableId.range(of: "^t(?:0|[1-9][0-9]*)$", options: .regularExpression) != nil,
                  referenced.insert(tableId).inserted, let table = tableRecords[tableId],
                  Set(table.keys) == ["tablePos", "sourceEnd", "rows", "columns", "columnWidths", "direction", "irregular", "readOnlyDescendants", "attrsKey", "sourceRows", "cells", "syntheticRegions", "failure", "compatibilityDiagnostic"],
                  let pos = number(table, "tablePos"), tableId == "t\(pos)", let end = number(table, "sourceEnd"),
                  let rows = number(table, "rows"), let columns = number(table, "columns"),
                  let widths = table["columnWidths"] as? [Any], let sourceRows = table["sourceRows"] as? [[String: Any]],
                  let cells = table["cells"] as? [[String: Any]], let synthetic = table["syntheticRegions"] as? [[String: Any]],
                  pos >= entry.start, end <= entry.end, end > pos, UInt64(widths.count) == columns,
                  table["direction"] is NSNull || ["ltr", "rtl"].contains(table["direction"] as? String ?? ""),
                  exactBool(table["irregular"]) != nil, exactBool(table["readOnlyDescendants"]) == (entry.depth > 0),
                  attrs(table["attrsKey"]),
                  table["failure"] is NSNull || failures.contains(table["failure"] as? String ?? ""),
                  table["compatibilityDiagnostic"] is NSNull || diagnostics.contains(table["compatibilityDiagnostic"] as? String ?? "")
            else { return false }
            if rows > 4_000_000 || columns > 4_000_000 { return false }
            slots += rows * columns
            if slots > 4_000_000 { return false }
            for width in widths where !(width is NSNull) {
                guard let value = v2ExactUInt32(width as? NSNumber), value > 0 else { return false }
            }
            if !(table["failure"] is NSNull) {
                if rows != 0 || columns != 0 || !cells.isEmpty || !sourceRows.isEmpty || !synthetic.isEmpty || !(table["compatibilityDiagnostic"] is NSNull) { return false }
                continue
            }
            nodes += sourceRows.count + cells.count + synthetic.count
            if nodes > 7_000_000 { return false }
            var rowEnd = pos + 1
            for row in sourceRows {
                guard Set(row.keys) == ["sourcePos", "sourceEnd", "attrsKey"],
                      let start = number(row, "sourcePos"), let finish = number(row, "sourceEnd"),
                      start >= rowEnd, finish > start, finish < end, attrs(row["attrsKey"])
                else { return false }
                rowEnd = finish
            }
            var occupied = Set<UInt64>()
            var cellEnd = pos + 1
            var sourceRowIndex = 0
            for (isSynthetic, regions) in [(false, cells), (true, synthetic)] {
                for region in regions {
                    var keys: Set<String> = ["row", "column", "rowspan", "colspan", "header", "attrsKey"]
                    if !isSynthetic { keys.formUnion(["sourcePos", "sourceEnd", "contentKey", "elements"]) }
                    guard Set(region.keys) == keys, let row = number(region, "row"), let column = number(region, "column"),
                          let rowspan = number(region, "rowspan"), let colspan = number(region, "colspan"),
                          rowspan > 0, colspan > 0, row + rowspan <= rows, column + colspan <= columns,
                          exactBool(region["header"]) != nil, attrs(region["attrsKey"])
                    else { return false }
                    for r in row..<(row + rowspan) {
                        for c in column..<(column + colspan) {
                            if !occupied.insert(r * columns + c).inserted { return false }
                        }
                    }
                    if isSynthetic { continue }
                    guard let start = number(region, "sourcePos"), let finish = number(region, "sourceEnd"),
                          let key = region["contentKey"] as? String, let elements = region["elements"] as? [Any],
                          start >= cellEnd, finish > start, !key.isEmpty
                    else { return false }
                    while sourceRowIndex < sourceRows.count, number(sourceRows[sourceRowIndex], "sourceEnd")! <= start { sourceRowIndex += 1 }
                    guard sourceRowIndex < sourceRows.count, number(sourceRows[sourceRowIndex], "sourcePos")! < start,
                          number(sourceRows[sourceRowIndex], "sourceEnd")! > finish else { return false }
                    cellEnd = finish
                    pending.append(contentsOf: elements.map { ($0, entry.depth + 1, start + 1, finish - 1) })
                }
            }
        }
        return !requireCompletePool || (referenced == Set(tableRecords.keys) && referencedAttributes == Set(tableAttributes.keys))
    }

    private static func isValidRenderBlocks(_ value: Any, tableAttributes: [String: [String: Any]] = [:], tableRecords: [String: [String: Any]] = [:], requireCompletePool: Bool = true) -> Bool {
        guard let blocks = value as? [Any] else { return false }
        var elements: [Any] = []
        for block in blocks {
            guard let values = block as? [Any] else { return false }
            elements.append(contentsOf: values)
        }
        return validSemanticRenderElements(elements, tableAttributes: tableAttributes, tableRecords: tableRecords, requireCompletePool: requireCompletePool)
    }

    private static func isValidRenderPatch(_ value: Any, tableAttributes: [String: [String: Any]] = [:], tableRecords: [String: [String: Any]] = [:]) -> Bool {
        if value is NSNull { return true }
        guard let object = value as? [String: Any],
              Set(object.keys) == [
                "baseDocumentVersion",
                "startIndex",
                "deleteCount",
                "renderBlocks"
              ],
              uint64Field(object, "baseDocumentVersion") != nil,
              uint32Field(object, "startIndex") != nil,
              uint32Field(object, "deleteCount") != nil,
              let renderBlocks = object["renderBlocks"],
              isValidRenderBlocks(renderBlocks, tableAttributes: tableAttributes, tableRecords: tableRecords, requireCompletePool: false)
        else {
            return false
        }
        return true
    }

    static func parseTableRecords(_ value: Any?) -> [String: [String: Any]]? {
        guard let raw = (value ?? [String: Any]()) as? [String: Any] else { return nil }
        var records: [String: [String: Any]] = [:]
        for (id, value) in raw {
            guard !id.isEmpty, let record = value as? [String: Any] else { return nil }
            records[id] = record
        }
        return records
    }

    private static func isBooleanRecord(_ value: Any?) -> Bool {
        guard let object = value as? [String: Any] else { return false }
        return object.values.allSatisfy { exactBool($0) != nil }
    }

    private static func isStringArray(_ value: Any?) -> Bool {
        guard let array = value as? [Any] else { return false }
        return array.allSatisfy { $0 is String }
    }

    private static func isValidActiveState(_ value: Any) -> Bool {
        guard let object = value as? [String: Any],
              Set(object.keys) == activeStateKeys,
              isBooleanRecord(object["marks"]),
              let markAttrs = object["markAttrs"] as? [String: Any],
              markAttrs.values.allSatisfy({ $0 is [String: Any] }),
              isBooleanRecord(object["nodes"]),
              isBooleanRecord(object["commands"]),
              isStringArray(object["allowedMarks"]),
              isStringArray(object["insertableNodes"])
        else {
            return false
        }
        return true
    }

    private static func scalarSelection(from value: Any) -> (anchor: UInt32, head: UInt32)? {
        guard let selection = value as? [String: Any],
              let type = selection["type"] as? String
        else {
            return nil
        }
        switch type {
        case "text":
            guard Set(selection.keys) == ["type", "anchor", "head", "anchorScalar", "headScalar"],
                  uint32Field(selection, "anchor") != nil,
                  uint32Field(selection, "head") != nil,
                  let anchor = uint32Field(selection, "anchorScalar"),
                  let head = uint32Field(selection, "headScalar")
            else {
                return nil
            }
            return (anchor, head)
        case "node":
            guard Set(selection.keys) == ["type", "pos", "posScalar"],
                  uint32Field(selection, "pos") != nil,
                  uint32Field(selection, "posScalar") != nil
            else {
                return nil
            }
            return nil
        case "all":
            return Set(selection.keys) == ["type"] ? nil : nil
        default:
            return nil
        }
    }

    private static func isValidSelection(_ value: Any) -> Bool {
        guard let selection = value as? [String: Any],
              let type = selection["type"] as? String
        else {
            return false
        }
        switch type {
        case "text":
            return scalarSelection(from: selection) != nil
        case "node":
            return Set(selection.keys) == ["type", "pos", "posScalar"]
                && uint32Field(selection, "pos") != nil
                && uint32Field(selection, "posScalar") != nil
        case "all":
            return Set(selection.keys) == ["type"]
        default:
            return false
        }
    }

    static func parseAtomicRenderSnapshot(_ json: String) -> AtomicRenderSnapshot? {
        guard let data = json.data(using: .utf8),
              var object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              Set(object.keys).isSubset(of: atomicRenderSnapshotKeys.union(["positionEpoch", "tableAttributes", "tableRecords"])),
              atomicRenderSnapshotKeys.isSubset(of: Set(object.keys)),
              let renderBlocks = object["renderBlocks"],
              let renderPatch = object["renderPatch"],
              let tableAttributes = parseTableAttributes(object["tableAttributes"]),
              let tableRecords = parseTableRecords(object["tableRecords"]),
              (isValidRenderBlocks(renderBlocks, tableAttributes: tableAttributes, tableRecords: tableRecords) && renderPatch is NSNull)
                || (renderBlocks is NSNull && !(renderPatch is NSNull) && isValidRenderPatch(renderPatch, tableAttributes: tableAttributes, tableRecords: tableRecords)),
              let selectionValue = object["selection"],
              isValidSelection(selectionValue),
              let activeState = object["activeState"] as? [String: Any],
              isValidActiveState(activeState),
              let history = object["historyState"] as? [String: Any],
              Set(history.keys) == ["canUndo", "canRedo"],
              let canUndo = exactBool(history["canUndo"]),
              let canRedo = exactBool(history["canRedo"]),
              let documentRevision = uint64Field(object, "documentVersion"),
              let stateRevision = uint64Field(object, "stateRevision"),
              let scalarLength = uint32Field(object, "scalarLength"),
              let documentIsEmpty = exactBool(object["documentIsEmpty"])
        else {
            return nil
        }

        let selection = scalarSelection(from: selectionValue)
        let positionEpoch: UInt64?
        if object.keys.contains("positionEpoch") {
            guard let value = object["positionEpoch"] as? String,
                  let parsed = UInt64(value), String(parsed) == value
            else {
                return nil
            }
            positionEpoch = parsed
        } else {
            positionEpoch = nil
        }
        object.removeValue(forKey: "positionEpoch")
        guard let atomicData = try? JSONSerialization.data(withJSONObject: object),
              let atomicRenderJSON = String(data: atomicData, encoding: .utf8)
        else {
            return nil
        }
        object.removeValue(forKey: "scalarLength")
        // documentIsEmpty stays in the view payload: the text view needs the
        // core's answer to decide whether to show its placeholder.
        guard let viewData = try? JSONSerialization.data(withJSONObject: object),
              let viewUpdateJSON = String(data: viewData, encoding: .utf8)
        else {
            return nil
        }
        return AtomicRenderSnapshot(
            renderObject: object,
            tableAttributes: tableAttributes,
            tableRecords: tableRecords,
            atomicRenderJSON: atomicRenderJSON,
            viewUpdateJSON: viewUpdateJSON,
            documentRevision: documentRevision,
            stateRevision: stateRevision,
            scalarLength: scalarLength,
            selection: selection,
            activeState: activeState,
            historyState: (canUndo, canRedo),
            documentIsEmpty: documentIsEmpty,
            positionEpoch: positionEpoch
        )
    }

}
