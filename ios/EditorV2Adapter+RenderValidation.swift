import Foundation

extension EditorV2Adapter {
    struct EditorTablePresentationSnapshot {
        let documentRevision: UInt64
        let positionEpoch: UInt64?
        let tableAttributes: [String: [String: Any]]
        let tableRecords: [String: FfiViewerTable]
        let tableInputMappings: TableInputMappings?
    }

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
        let tableInputMappings: TableInputMappings?
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

    struct TableInputExtent: Equatable {
        let scalarStart: UInt32
        let scalarEnd: UInt32
    }

    struct TableInputBlock: Equatable {
        let elementIndex: UInt32
        let docStart: UInt32
        let docEnd: UInt32
        let scalarStart: UInt32
        let contentScalarStart: UInt32
        let scalarEnd: UInt32
        let breakScalarEnd: UInt32
        let isVoid: Bool
    }

    struct TableInputExcluded: Equatable {
        let elementIndex: UInt32
        let tableID: String
        let extent: TableInputExtent?
    }

    struct TableInputCell: Equatable {
        let cellIndex: UInt32
        let sourcePos: UInt32
        let sourceEnd: UInt32
        let blocks: [TableInputBlock]
        let excluded: [TableInputExcluded]
    }

    struct TableInputTable: Equatable {
        let extent: TableInputExtent?
        let cells: [TableInputCell]
    }

    struct TableInputMappings: Equatable {
        let tables: [String: TableInputTable]
    }

    private static func jsonString(_ value: Any) -> String? {
        guard JSONSerialization.isValidJSONObject(value),
              let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    private static func lowerRenderMark(_ value: Any) -> FfiViewerMark? {
        if let markType = value as? String {
            return FfiViewerMark(markType: markType, attrsJson: "{}")
        }
        guard var object = value as? [String: Any], let markType = object.removeValue(forKey: "type") as? String,
              let attrsJSON = jsonString(object)
        else {
            return nil
        }
        return FfiViewerMark(markType: markType, attrsJson: attrsJSON)
    }

    private static func lowerRenderElements(_ values: [Any]) -> [FfiViewerElement]? {
        let elements = values.compactMap { value -> FfiViewerElement? in
            guard let object = value as? [String: Any], let type = object["type"] as? String else { return nil }
            switch type {
            case "table":
                guard let tableID = object["tableId"] as? String else { return nil }
                return .table(tableId: tableID)
            case "textRun":
                guard let text = object["text"] as? String, let values = object["marks"] as? [Any] else { return nil }
                let marks = values.compactMap(lowerRenderMark)
                guard marks.count == values.count else { return nil }
                return .textRun(text: text, marks: marks)
            case "voidInline", "opaqueInlineAtom", "voidBlock", "opaqueBlockAtom":
                guard let nodeType = object["nodeType"] as? String, let docPos = uint32Field(object, "docPos") else { return nil }
                let attrs = object["attrs"] as? [String: Any] ?? [:]
                guard let attrsJSON = jsonString(attrs) else { return nil }
                let label: String
                if type == "voidInline" || type == "voidBlock" {
                    let base = (attrs["label"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? nodeType
                    if nodeType == "mention", let trigger = attrs["mentionSuggestionChar"] as? String,
                       !trigger.isEmpty, !base.hasPrefix(trigger) {
                        label = trigger + base
                    } else {
                        label = base
                    }
                } else {
                    guard let explicit = object["label"] as? String else { return nil }
                    label = explicit
                }
                return (type == "voidInline" || type == "opaqueInlineAtom")
                    ? .inlineAtom(nodeType: nodeType, docPos: docPos, attrsJson: attrsJSON, label: label)
                    : .blockAtom(nodeType: nodeType, docPos: docPos, attrsJson: attrsJSON, label: label)
            case "blockStart":
                guard let nodeType = object["nodeType"] as? String, let depth = uint32Field(object, "depth"),
                      let typedDepth = UInt16(exactly: depth)
                else { return nil }
                let language = object["language"] as? String
                let listContextJSON: String?
                if var context = object["listContext"] as? [String: Any] {
                    context["kind"] = context["kind"] ?? NSNull()
                    context["checked"] = context["checked"] ?? NSNull()
                    listContextJSON = jsonString(context)
                } else {
                    listContextJSON = nil
                }
                if object["listContext"] != nil && listContextJSON == nil { return nil }
                return .blockStart(nodeType: nodeType, language: language, depth: typedDepth, listContextJson: listContextJSON)
            case "blockEnd":
                return .blockEnd
            default:
                return nil
            }
        }
        return elements.count == values.count ? elements : nil
    }

    private static func lowerTableRecord(_ record: [String: Any]) -> FfiViewerTable? {
        guard let tablePos = uint32Field(record, "tablePos"), let sourceEnd = uint32Field(record, "sourceEnd"),
              let rows = uint32Field(record, "rows"), let columns = uint32Field(record, "columns"),
              let columnWidths = record["columnWidths"] as? [Any], let irregular = exactBool(record["irregular"]),
              let readOnlyDescendants = exactBool(record["readOnlyDescendants"]), let attrsKey = record["attrsKey"] as? String,
              let rawRows = record["sourceRows"] as? [[String: Any]], let rawCells = record["cells"] as? [[String: Any]],
              let rawSyntheticRegions = record["syntheticRegions"] as? [[String: Any]]
        else { return nil }
        let widths = columnWidths.map { value -> UInt32? in
            value is NSNull ? nil : v2ExactUInt32(value as? NSNumber)
        }
        guard widths.count == columnWidths.count else { return nil }
        let sourceRows = rawRows.compactMap { row -> TableRenderRow? in
            guard let sourcePos = uint32Field(row, "sourcePos"), let sourceEnd = uint32Field(row, "sourceEnd"),
                  let attrsKey = row["attrsKey"] as? String else { return nil }
            return TableRenderRow(sourcePos: sourcePos, sourceEnd: sourceEnd, attrsKey: attrsKey)
        }
        guard sourceRows.count == rawRows.count else { return nil }
        let cells = rawCells.compactMap { cell -> FfiViewerTableCell? in
            guard let sourcePos = uint32Field(cell, "sourcePos"), let sourceEnd = uint32Field(cell, "sourceEnd"),
                  let row = uint32Field(cell, "row"), let column = uint32Field(cell, "column"),
                  let rowspan = uint32Field(cell, "rowspan"), let colspan = uint32Field(cell, "colspan"),
                  let header = exactBool(cell["header"]), let attrsKey = cell["attrsKey"] as? String,
                  let contentKey = cell["contentKey"] as? String, let rawElements = cell["elements"] as? [Any],
                  let elements = lowerRenderElements(rawElements)
            else { return nil }
            return FfiViewerTableCell(sourcePos: sourcePos, sourceEnd: sourceEnd, row: row, column: column,
                                      rowspan: rowspan, colspan: colspan, header: header, attrsKey: attrsKey,
                                      contentKey: contentKey, elements: elements)
        }
        guard cells.count == rawCells.count else { return nil }
        let syntheticRegions = rawSyntheticRegions.compactMap { region -> TableRenderSyntheticRegion? in
            guard let row = uint32Field(region, "row"), let column = uint32Field(region, "column"),
                  let rowspan = uint32Field(region, "rowspan"), let colspan = uint32Field(region, "colspan"),
                  let header = exactBool(region["header"]), let attrsKey = region["attrsKey"] as? String
            else { return nil }
            return TableRenderSyntheticRegion(row: row, column: column, rowspan: rowspan, colspan: colspan,
                                              header: header, attrsKey: attrsKey)
        }
        guard syntheticRegions.count == rawSyntheticRegions.count else { return nil }
        let failure: TableRenderFailure?
        switch record["failure"] as? String {
        case nil: failure = nil
        case "gridLimit": failure = .gridLimit
        case "workLimit": failure = .workLimit
        case "allocation": failure = .allocation
        case "invalidStructure": failure = .invalidStructure
        case "invalidAttributes": failure = .invalidAttributes
        default: return nil
        }
        let diagnostic: TableCompatibilityDiagnostic?
        switch record["compatibilityDiagnostic"] as? String {
        case nil: diagnostic = nil
        case "virtual-grid-limit": diagnostic = .virtualGridLimit
        case "empty-reference-surface": diagnostic = .emptyReferenceSurface
        case "unsupported-row-role": diagnostic = .unsupportedRowRole
        case "unsupported-cell-role": diagnostic = .unsupportedCellRole
        case "ambiguous-source-map": diagnostic = .ambiguousSourceMap
        case "unsupported-gap-default": diagnostic = .unsupportedGapDefault
        case "overlapping-reference-cells": diagnostic = .overlappingReferenceCells
        case "unmapped-reference-cell": diagnostic = .unmappedReferenceCell
        case "nonrectangular-reference-cell": diagnostic = .nonrectangularReferenceCell
        case "zero-span-after-reference-pass": diagnostic = .zeroSpanAfterReferencePass
        default: return nil
        }
        return FfiViewerTable(tablePos: tablePos, sourceEnd: sourceEnd, rows: rows, columns: columns,
                              columnWidths: widths, direction: record["direction"] as? String, irregular: irregular,
                              readOnlyDescendants: readOnlyDescendants, attrsKey: attrsKey, sourceRows: sourceRows,
                              cells: cells, syntheticRegions: syntheticRegions, failure: failure,
                              compatibilityDiagnostic: diagnostic)
    }

    static func lowerTablePresentation(
        from snapshot: AtomicRenderSnapshot,
        positionEpoch: UInt64?
    ) -> EditorTablePresentationSnapshot? {
        guard !snapshot.tableRecords.isEmpty else { return nil }
        var tableRecords: [String: FfiViewerTable] = [:]
        for (tableID, record) in snapshot.tableRecords {
            guard let table = lowerTableRecord(record) else { return nil }
            tableRecords[tableID] = table
        }
        return EditorTablePresentationSnapshot(
            documentRevision: snapshot.documentRevision,
            positionEpoch: positionEpoch,
            tableAttributes: snapshot.tableAttributes,
            tableRecords: tableRecords,
            tableInputMappings: snapshot.tableInputMappings
        )
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

    private static func parseTableInputExtent(_ value: Any, scalarLength: UInt32) -> TableInputExtent? {
        guard let object = value as? [String: Any], Set(object.keys) == ["scalarStart", "scalarEnd"],
              let scalarStart = uint32Field(object, "scalarStart"),
              let scalarEnd = uint32Field(object, "scalarEnd"),
              scalarStart <= scalarEnd, scalarEnd <= scalarLength
        else { return nil }
        return TableInputExtent(scalarStart: scalarStart, scalarEnd: scalarEnd)
    }

    private static func hasValidCompleteTablePool(
        _ tableAttributes: [String: [String: Any]],
        _ tableRecords: [String: [String: Any]]
    ) -> Bool {
        let roots: [[String: Any]] = tableRecords.compactMap { id, record in
            exactBool(record["readOnlyDescendants"]) == false ? ["type": "table", "tableId": id] : nil
        }
        return !roots.isEmpty && validSemanticRenderElements(
            roots, tableAttributes: tableAttributes, tableRecords: tableRecords
        )
    }

    static func parseTableInputMappings(
        _ value: Any,
        tableAttributes: [String: [String: Any]],
        tableRecords: [String: [String: Any]],
        scalarLength: UInt32
    ) -> TableInputMappings? {
        guard hasValidCompleteTablePool(tableAttributes, tableRecords),
              let root = value as? [String: Any], Set(root.keys) == ["version", "tables"],
              uint32Field(root, "version") == 1,
              let rawTables = root["tables"] as? [String: Any],
              Set(rawTables.keys) == Set(tableRecords.keys)
        else { return nil }

        var tables: [String: TableInputTable] = [:]
        for (tableID, record) in tableRecords {
            guard let rawTable = rawTables[tableID] as? [String: Any],
                  Set(rawTable.keys) == ["extent", "cells"],
                  let rawCells = rawTable["cells"] as? [[String: Any]],
                  let recordCells = record["cells"] as? [[String: Any]],
                  rawCells.count == recordCells.count
            else { return nil }
            let extent: TableInputExtent?
            if rawTable["extent"] is NSNull {
                extent = nil
            } else {
                guard let rawExtent = rawTable["extent"], let parsed = parseTableInputExtent(rawExtent, scalarLength: scalarLength) else { return nil }
                extent = parsed
            }
            var cells: [TableInputCell] = []
            var observedTableExtent: TableInputExtent?
            for (index, rawCell) in rawCells.enumerated() {
                guard Set(rawCell.keys) == ["cellIndex", "sourcePos", "sourceEnd", "blocks", "excluded"],
                      let cellIndex = uint32Field(rawCell, "cellIndex"), cellIndex == UInt32(index),
                      let sourcePos = uint32Field(rawCell, "sourcePos"),
                      let sourceEnd = uint32Field(rawCell, "sourceEnd"),
                      sourcePos == uint32Field(recordCells[index], "sourcePos"),
                      sourceEnd == uint32Field(recordCells[index], "sourceEnd"), sourcePos < sourceEnd,
                      let rawBlocks = rawCell["blocks"] as? [[String: Any]],
                      let rawExcluded = rawCell["excluded"] as? [[String: Any]],
                      let elements = recordCells[index]["elements"] as? [[String: Any]]
                else { return nil }

                let expectedExcluded = elements.enumerated().compactMap { offset, element -> (Int, String)? in
                    guard element["type"] as? String == "table", let tableID = element["tableId"] as? String else { return nil }
                    return (offset, tableID)
                }
                guard rawExcluded.count == expectedExcluded.count else { return nil }

                var blocks: [TableInputBlock] = []
                for (blockIndex, rawBlock) in rawBlocks.enumerated() {
                    guard Set(rawBlock.keys) == ["elementIndex", "docStart", "docEnd", "scalarStart", "contentScalarStart", "scalarEnd", "breakScalarEnd", "void"],
                          let elementIndex = uint32Field(rawBlock, "elementIndex"), Int(elementIndex) < elements.count,
                          let docStart = uint32Field(rawBlock, "docStart"), let docEnd = uint32Field(rawBlock, "docEnd"),
                          let scalarStart = uint32Field(rawBlock, "scalarStart"), let contentScalarStart = uint32Field(rawBlock, "contentScalarStart"),
                          let scalarEnd = uint32Field(rawBlock, "scalarEnd"), let breakScalarEnd = uint32Field(rawBlock, "breakScalarEnd"),
                          let isVoid = exactBool(rawBlock["void"]),
                          sourcePos < docStart, docStart <= docEnd, docEnd < sourceEnd,
                          scalarStart <= contentScalarStart, contentScalarStart <= scalarEnd, scalarEnd <= breakScalarEnd,
                          breakScalarEnd <= scalarLength, breakScalarEnd - scalarEnd <= 1,
                          (blockIndex == 0 || blocks.last!.elementIndex < elementIndex)
                    else { return nil }
                    let element = elements[Int(elementIndex)]
                    if isVoid {
                        guard ["voidBlock", "opaqueBlockAtom"].contains(element["type"] as? String ?? ""),
                              docStart == docEnd, uint32Field(element, "docPos") == docStart else { return nil }
                    } else if element["type"] as? String != "blockStart" {
                        return nil
                    }
                    blocks.append(.init(elementIndex: elementIndex, docStart: docStart, docEnd: docEnd,
                                        scalarStart: scalarStart, contentScalarStart: contentScalarStart,
                                        scalarEnd: scalarEnd, breakScalarEnd: breakScalarEnd, isVoid: isVoid))
                }

                var excluded: [TableInputExcluded] = []
                for (excludedIndex, rawExcludedEntry) in rawExcluded.enumerated() {
                    guard Set(rawExcludedEntry.keys) == ["elementIndex", "tableId", "extent"],
                          let elementIndex = uint32Field(rawExcludedEntry, "elementIndex"), elementIndex == UInt32(expectedExcluded[excludedIndex].0),
                          let nestedID = rawExcludedEntry["tableId"] as? String, nestedID == expectedExcluded[excludedIndex].1,
                          let nestedRecord = tableRecords[nestedID],
                          let nestedStart = uint32Field(nestedRecord, "tablePos"), let nestedEnd = uint32Field(nestedRecord, "sourceEnd"),
                          sourcePos < nestedStart, nestedStart < nestedEnd, nestedEnd < sourceEnd
                    else { return nil }
                    let nestedExtent: TableInputExtent?
                    if rawExcludedEntry["extent"] is NSNull {
                        nestedExtent = nil
                    } else {
                        guard let rawExtent = rawExcludedEntry["extent"], let parsed = parseTableInputExtent(rawExtent, scalarLength: scalarLength) else { return nil }
                        nestedExtent = parsed
                    }
                    excluded.append(.init(elementIndex: elementIndex, tableID: nestedID, extent: nestedExtent))
                }

                let ordered = blocks.map { (index: $0.elementIndex, docStart: $0.docStart, docEnd: $0.docEnd, scalarStart: Optional($0.scalarStart), scalarEnd: Optional($0.scalarEnd), breakEnd: Optional($0.breakScalarEnd)) }
                    + excluded.map { excluded in
                        let nested = tableRecords[excluded.tableID]!
                        return (index: excluded.elementIndex, docStart: uint32Field(nested, "tablePos")!, docEnd: uint32Field(nested, "sourceEnd")!, scalarStart: excluded.extent?.scalarStart, scalarEnd: excluded.extent?.scalarEnd, breakEnd: excluded.extent?.scalarEnd)
                    }
                let sourceOrdered = ordered.sorted { $0.index < $1.index }
                for pair in zip(sourceOrdered, sourceOrdered.dropFirst()) {
                    guard pair.0.index < pair.1.index, pair.0.docEnd <= pair.1.docStart else { return nil }
                    if let previousEnd = pair.0.breakEnd, let nextStart = pair.1.scalarStart {
                        guard previousEnd <= nextStart else { return nil }
                    }
                }
                var cellExtent: TableInputExtent?
                var cellBreakEnd: UInt32?
                for part in sourceOrdered {
                    guard let start = part.scalarStart, let end = part.scalarEnd else { continue }
                    if let cellBreakEnd { guard cellBreakEnd <= start else { return nil } }
                    cellExtent = .init(scalarStart: cellExtent?.scalarStart ?? start, scalarEnd: end)
                    cellBreakEnd = part.breakEnd
                }
                if let cellExtent {
                    guard (cellBreakEnd ?? 0) <= cellExtent.scalarEnd else { return nil }
                    if let previous = observedTableExtent { guard previous.scalarEnd <= cellExtent.scalarStart else { return nil } }
                    observedTableExtent = .init(scalarStart: observedTableExtent?.scalarStart ?? cellExtent.scalarStart, scalarEnd: cellExtent.scalarEnd)
                }
                cells.append(.init(cellIndex: cellIndex, sourcePos: sourcePos, sourceEnd: sourceEnd, blocks: blocks, excluded: excluded))
            }
            if record["failure"] is NSNull, observedTableExtent != extent { return nil }
            tables[tableID] = .init(extent: extent, cells: cells)
        }
        for table in tables.values {
            for cell in table.cells {
                for excluded in cell.excluded {
                    guard tables[excluded.tableID]?.extent == excluded.extent else { return nil }
                }
            }
        }
        let rootExtents = tableRecords.compactMap { tableID, record -> (UInt32, TableInputExtent)? in
            guard exactBool(record["readOnlyDescendants"]) == false,
                  let sourcePos = uint32Field(record, "tablePos"), let extent = tables[tableID]?.extent else { return nil }
            return (sourcePos, extent)
        }.sorted { $0.0 < $1.0 }
        for pair in zip(rootExtents, rootExtents.dropFirst()) {
            guard pair.0.1.scalarEnd <= pair.1.1.scalarStart else { return nil }
        }
        return TableInputMappings(tables: tables)
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
              Set(object.keys).isSubset(of: atomicRenderSnapshotKeys.union(["positionEpoch", "tableAttributes", "tableRecords", "tableInputMappings"])),
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

        let tableInputMappings: TableInputMappings?
        if let rawTableInputMappings = object["tableInputMappings"] {
            guard let parsed = parseTableInputMappings(
                rawTableInputMappings,
                tableAttributes: tableAttributes,
                tableRecords: tableRecords,
                scalarLength: scalarLength
            ) else { return nil }
            tableInputMappings = parsed
        } else {
            tableInputMappings = nil
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
            tableInputMappings: tableInputMappings,
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
