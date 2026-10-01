import UIKit

func accessibilityScreenRect(_ rect: CGRect, in view: UIView) -> CGRect {
    guard let screen = view.window?.screen.coordinateSpace,
          !rect.isNull, !rect.isEmpty,
          [rect.minX, rect.minY, rect.width, rect.height].allSatisfy(\.isFinite)
    else { return .zero }
    return view.convert(rect, to: screen)
}

enum TableAccessibilityText {
    case table
    case emptyCell
    case emptyTable
    case failedTable
    case rowSpan(Int)
    case columnSpan(Int)
    case openLink(String)
    case openMention(String)

    var localized: String {
        switch self {
        case .table:
            return Self.string("table.accessibility.table", "Table")
        case .emptyCell:
            return Self.string("table.accessibility.emptyCell", "Empty cell")
        case .emptyTable:
            return Self.string("table.accessibility.emptyTable", "Empty table")
        case .failedTable:
            return Self.string("table.accessibility.failedTable", "Table could not be displayed")
        case let .rowSpan(count):
            return String(format: Self.string("table.accessibility.rowSpan", "Spans %d rows"), count)
        case let .columnSpan(count):
            return String(format: Self.string("table.accessibility.columnSpan", "Spans %d columns"), count)
        case let .openLink(label):
            return String(format: Self.string("table.accessibility.openLink", "Open link %@"), label)
        case let .openMention(label):
            return String(format: Self.string("table.accessibility.openMention", "Open mention %@"), label)
        }
    }

    static func string(_ key: String, _ value: String) -> String {
        NSLocalizedString(key, tableName: nil, bundle: Bundle(for: TableAccessibilityTableElement.self),
                          value: value, comment: "")
    }
}

struct TableAccessibilityAction: Equatable {
    let key: String
    let applicability: String
    let command: [String: String]
    let defaultLabel: String

    var label: String { TableAccessibilityText.string("table.accessibility.action.\(key)", defaultLabel) }

    static let deleteTable = TableAccessibilityAction(
        key: "deleteTable", applicability: "deleteTable", command: ["type": "deleteTable"],
        defaultLabel: "Delete table"
    )

    static let all: [TableAccessibilityAction] = [
        TableAccessibilityAction(key: "addRowBefore", applicability: "addTableRowBefore",
                                 command: ["type": "addTableRow", "side": "before"], defaultLabel: "Insert row above"),
        TableAccessibilityAction(key: "addRowAfter", applicability: "addTableRowAfter",
                                 command: ["type": "addTableRow", "side": "after"], defaultLabel: "Insert row below"),
        TableAccessibilityAction(key: "deleteRows", applicability: "deleteTableRows",
                                 command: ["type": "deleteTableRows"], defaultLabel: "Delete row"),
        TableAccessibilityAction(key: "selectRows", applicability: "selectTableRows",
                                 command: ["type": "selectTableRows"], defaultLabel: "Select row"),
        TableAccessibilityAction(key: "addColumnBefore", applicability: "addTableColumnBefore",
                                 command: ["type": "addTableColumn", "side": "before"],
                                 defaultLabel: "Insert column before"),
        TableAccessibilityAction(key: "addColumnAfter", applicability: "addTableColumnAfter",
                                 command: ["type": "addTableColumn", "side": "after"],
                                 defaultLabel: "Insert column after"),
        TableAccessibilityAction(key: "deleteColumns", applicability: "deleteTableColumns",
                                 command: ["type": "deleteTableColumns"], defaultLabel: "Delete column"),
        TableAccessibilityAction(key: "selectColumns", applicability: "selectTableColumns",
                                 command: ["type": "selectTableColumns"], defaultLabel: "Select column"),
        TableAccessibilityAction(key: "toggleHeaderRow", applicability: "toggleTableHeaderRow",
                                 command: ["type": "toggleTableHeader", "target": "row"],
                                 defaultLabel: "Toggle header row"),
        TableAccessibilityAction(key: "toggleHeaderColumn", applicability: "toggleTableHeaderColumn",
                                 command: ["type": "toggleTableHeader", "target": "column"],
                                 defaultLabel: "Toggle header column"),
        TableAccessibilityAction(key: "toggleHeaderCell", applicability: "toggleTableHeaderCell",
                                 command: ["type": "toggleTableHeader", "target": "cell"],
                                 defaultLabel: "Toggle header cell"),
        TableAccessibilityAction(key: "mergeCells", applicability: "mergeTableCells",
                                 command: ["type": "mergeTableCells"], defaultLabel: "Merge cells"),
        TableAccessibilityAction(key: "splitCell", applicability: "splitTableCell",
                                 command: ["type": "splitTableCell"], defaultLabel: "Split cell"),
        TableAccessibilityAction(key: "clearCells", applicability: "clearTableCells",
                                 command: ["type": "clearTableCells"], defaultLabel: "Clear cells"),
        deleteTable
    ]
}

struct TableAccessibilityCell {
    let surface: ViewerTableSurface
    let cell: PreparedViewerTableCell
    let rows: NSRange
    let columns: NSRange
    let isHeader: Bool
    let label: String
    let containsLink: Bool

    var sourceIndex: Int { cell.sourceIndex }

    var spanDescription: String? {
        let parts = [
            rows.length > 1 ? TableAccessibilityText.rowSpan(rows.length).localized : nil,
            columns.length > 1 ? TableAccessibilityText.columnSpan(columns.length).localized : nil
        ].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: TableAccessibility.descriptionSeparator)
    }
}

struct TableAccessibilityTable {
    enum Frame {
        case empty
        case failed
    }

    let surface: ViewerTableSurface
    let rowCount: Int
    let columnCount: Int
    let cells: [TableAccessibilityCell]
    private let slots: [Int: Int]
    private let positions: [Int: Int]
    private let columnHeaderCells: [Int: [Int]]
    private let rowHeaderCells: [Int: [Int]]

    init(surface: ViewerTableSurface, rowCount: Int, columnCount: Int, cells: [TableAccessibilityCell]) {
        self.surface = surface
        self.rowCount = rowCount
        self.columnCount = columnCount
        self.cells = cells
        let rowStride = max(columnCount, 1)
        var slots: [Int: Int] = [:]
        for (index, cell) in cells.enumerated() {
            for row in cell.rows.location..<NSMaxRange(cell.rows) {
                for column in cell.columns.location..<NSMaxRange(cell.columns) where slots[row * rowStride + column] == nil {
                    slots[row * rowStride + column] = index
                }
            }
        }
        self.slots = slots
        positions = Dictionary(cells.enumerated().map { ($1.sourceIndex, $0) }, uniquingKeysWith: { first, _ in first })
        func headerLines(_ line: (TableAccessibilityCell) -> Int) -> Set<Int> {
            Set(Dictionary(grouping: cells, by: line).filter { $0.value.allSatisfy(\.isHeader) }.keys)
        }
        func headerCells(in lines: Set<Int>, line: (TableAccessibilityCell) -> Int,
                         covered: (TableAccessibilityCell) -> NSRange) -> [Int: [Int]] {
            var headers: [Int: [Int]] = [:]
            for (index, cell) in cells.enumerated() where cell.isHeader && lines.contains(line(cell)) {
                let range = covered(cell)
                for crossing in range.location..<NSMaxRange(range) { headers[crossing, default: []].append(index) }
            }
            return headers
        }
        columnHeaderCells = headerCells(in: headerLines { $0.rows.location }, line: { $0.rows.location }, covered: \.columns)
        rowHeaderCells = headerCells(in: headerLines { $0.columns.location }, line: { $0.columns.location }, covered: \.rows)
    }

    var identity: String { surface.identity }

    var frame: Frame? {
        guard cells.isEmpty else { return nil }
        let unfilled = surface.sourceTable?.failure == nil && (rowCount == 0 || columnCount == 0)
        return unfilled ? .empty : .failed
    }

    func cellIndex(row: Int, column: Int) -> Int? {
        guard (0..<max(columnCount, 1)).contains(column) else { return nil }
        return slots[row * max(columnCount, 1) + column]
    }

    func cellIndex(sourceIndex: Int) -> Int? {
        positions[sourceIndex]
    }

    func cell(row: Int, column: Int) -> TableAccessibilityCell? {
        cellIndex(row: row, column: column).map { cells[$0] }
    }

    func columnHeaders(_ column: Int) -> [TableAccessibilityCell] {
        columnHeaderCells[column, default: []].map { cells[$0] }
    }

    func rowHeaders(_ row: Int) -> [TableAccessibilityCell] {
        rowHeaderCells[row, default: []].map { cells[$0] }
    }
}

enum TableAccessibilityStructure: Equatable {
    struct Cell: Equatable {
        let rows: NSRange
        let columns: NSRange
        let isHeader: Bool
    }

    case node(role: PreparedProseAccessibilityNode.Role)
    case table(rows: Int, columns: Int, frame: TableAccessibilityTable.Frame?, cells: [Cell])
    case detachedFrame(TableAccessibilityTable.Frame)
}

enum TableAccessibilityItem {
    case node(ViewerTablePresentedAccessibilityNode)
    case table(TableAccessibilityTable)
    case detachedFrame(TableAccessibilityDetachedFrame)
}

enum TableAccessibility {
    static let labelSeparator = " "
    static let descriptionSeparator = ", "

    static func items(
        root: PreparedProseLayout,
        rootNodes: [ViewerTablePresentedAccessibilityNode],
        linksEnabled: Bool,
        detachedFrames: [TableAccessibilityDetachedFrame],
        tableDocumentPosition: ((String) -> UInt32?)?
    ) -> [TableAccessibilityItem] {
        var pendingFrames = detachedFrames.sorted { $0.tablePos < $1.tablePos }
        var merged: [TableAccessibilityItem] = []
        func flushFrames(before tablePos: UInt32?) {
            while let next = pendingFrames.first, tablePos.map({ next.tablePos < $0 }) ?? true {
                merged.append(.detachedFrame(next))
                pendingFrames.removeFirst()
            }
        }
        for item in drawnItems(root: root, rootNodes: rootNodes, linksEnabled: linksEnabled) {
            if case let .table(table) = item { flushFrames(before: tableDocumentPosition?(table.identity) ?? UInt32.max) }
            merged.append(item)
        }
        flushFrames(before: nil)
        return merged
    }

    private static func drawnItems(
        root: PreparedProseLayout,
        rootNodes: [ViewerTablePresentedAccessibilityNode],
        linksEnabled: Bool
    ) -> [TableAccessibilityItem] {
        var pending = root.blocks.enumerated().compactMap { blockIndex, block in
            block.tableSurface.map { (blockIndex, table($0, linksEnabled: linksEnabled)) }
        }
        var items: [TableAccessibilityItem] = []
        func flushTables(before blockIndex: Int) {
            while let next = pending.first, next.0 < blockIndex {
                items.append(.table(next.1))
                pending.removeFirst()
            }
        }
        for node in rootNodes {
            flushTables(before: node.node.sourceBlockIndex ?? Int.max)
            items.append(.node(node))
        }
        flushTables(before: Int.max)
        return items
    }

    static func contentSummary(of layout: PreparedProseLayout) -> [TableCellAccessibilitySummary] {
        let byBlock = Dictionary(grouping: layout.accessibilityNodes.indices.compactMap { index in
            layout.accessibilityNodes[index].sourceBlockIndex.map { ($0, index) }
        }, by: { $0.0 })
        var emitted = Set<Int>()
        var nodes: [TableCellAccessibilitySummary] = []
        func append(_ index: Int) {
            guard emitted.insert(index).inserted else { return }
            nodes.append(TableCellAccessibilitySummary(layout.accessibilityNodes[index]))
        }
        for (blockIndex, block) in layout.blocks.enumerated() {
            for (_, index) in byBlock[blockIndex] ?? [] { append(index) }
            block.tableSurface?.cells.forEach { nodes.append(contentsOf: $0.accessibilitySummary) }
        }
        layout.accessibilityNodes.indices.forEach(append)
        return nodes
    }

    private static func table(_ surface: ViewerTableSurface, linksEnabled: Bool) -> TableAccessibilityTable {
        let source = surface.sourceTable
        let accessibleCells = surface.cells.compactMap { cell -> TableAccessibilityCell? in
            let index = cell.sourceIndex
            guard let sourceCell = source?.cells[index]
            else { return nil }
            let nodes = cell.accessibilitySummary
            return TableAccessibilityCell(
                surface: surface,
                cell: cell,
                rows: NSRange(location: Int(sourceCell.row), length: Int(sourceCell.rowspan)),
                columns: NSRange(location: Int(sourceCell.column), length: Int(sourceCell.colspan)),
                isHeader: sourceCell.header,
                label: nodes.map(\.label).filter { !$0.isEmpty }.joined(separator: labelSeparator),
                containsLink: linksEnabled && nodes.contains { $0.role == .link && $0.interactionIndex != nil }
            )
        }
        return TableAccessibilityTable(
            surface: surface,
            rowCount: source.map { Int($0.rows) } ?? max(0, surface.layout.rowOffsets.count - 1),
            columnCount: source.map { Int($0.columns) } ?? surface.layout.columnWidths.count,
            cells: accessibleCells
        )
    }

    static func structure(of items: [TableAccessibilityItem]) -> [TableAccessibilityStructure] {
        items.map { item in
            switch item {
            case let .node(node):
                return .node(role: node.node.role)
            case let .table(table):
                return .table(rows: table.rowCount, columns: table.columnCount, frame: table.frame,
                              cells: table.cells.map {
                                  TableAccessibilityStructure.Cell(rows: $0.rows, columns: $0.columns, isHeader: $0.isHeader)
                              })
            case let .detachedFrame(frame):
                return .detachedFrame(frame.frame)
            }
        }
    }

    static func customActions(
        for cell: TableAccessibilityCell,
        tableID: String,
        editing: TableAccessibilityEditing?
    ) -> [UIAccessibilityCustomAction] {
        guard let editing else { return [] }
        return editing.tableAccessibilityActions(for: cell, tableID: tableID).map { action in
            UIAccessibilityCustomAction(name: action.label) { [weak editing] _ in
                editing?.performTableAccessibilityAction(action, for: cell, tableID: tableID) ?? false
            }
        }
    }
}

protocol TableAccessibilityEditing: AnyObject {
    func tableAccessibilityActions(for cell: TableAccessibilityCell, tableID: String) -> [TableAccessibilityAction]
    func performTableAccessibilityAction(_ action: TableAccessibilityAction, for cell: TableAccessibilityCell,
                                         tableID: String) -> Bool
    func activateTableAccessibilityCell(_ cell: TableAccessibilityCell, tableID: String) -> Bool
    func activeTableAccessibilityElement(for cell: TableAccessibilityCell, tableID: String) -> TableCellInputTextView?
    func detachedTableAccessibilityFrames() -> [TableAccessibilityDetachedFrame]
    func canDeleteTableAccessibilityFrame(tableID: String) -> Bool
    func deleteTableAccessibilityFrame(tableID: String) -> Bool
}

struct TableAccessibilityActiveCell {
    let cell: () -> TableAccessibilityCell?
    let actions: () -> [UIAccessibilityCustomAction]
}

struct TableAccessibilityDetachedFrame {
    let tableID: String
    let tablePos: UInt32
    let frame: TableAccessibilityTable.Frame
    let screenFrame: () -> CGRect
}

final class TableCellInputTextView: EditorTextView, UIAccessibilityContainerDataTableCell {
    var tableAccessibilityCell: TableAccessibilityActiveCell?

    override func becomeFirstResponder() -> Bool {
        let focused = super.becomeFirstResponder()
        if focused { onSelectionOrContentMayChange?() }
        return focused
    }

    override func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
        super.setMarkedText(markedText, selectedRange: selectedRange)
        onSelectionOrContentMayChange?()
    }

    override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get { tableAccessibilityCell.map { $0.actions() } ?? super.accessibilityCustomActions }
        set { super.accessibilityCustomActions = newValue }
    }

    func accessibilityRowRange() -> NSRange {
        tableAccessibilityCell?.cell()?.rows ?? NSRange(location: NSNotFound, length: 0)
    }

    func accessibilityColumnRange() -> NSRange {
        tableAccessibilityCell?.cell()?.columns ?? NSRange(location: NSNotFound, length: 0)
    }
}

class TableAccessibilityGeneratedElement: UIAccessibilityElement {
    weak var drawingView: PreparedProseDrawingView?

    init(drawingView: PreparedProseDrawingView, container: Any) {
        self.drawingView = drawingView
        super.init(accessibilityContainer: container)
    }

    var isCurrent: Bool { drawingView?.isLiveAccessibilityElement(self) == true }

    func screenFrame(_ rect: CGRect, clip: CGRect) -> CGRect {
        guard let drawingView else { return .zero }
        return drawingView.accessibilityScreenFrame(rect, clip: clip)
    }
}

final class TableAccessibilityTableElement: TableAccessibilityGeneratedElement, UIAccessibilityContainerDataTable {
    private(set) var table: TableAccessibilityTable
    private var materializedCellElements: [Int: TableAccessibilityCellElement] = [:]

    init(drawingView: PreparedProseDrawingView, table: TableAccessibilityTable) {
        self.table = table
        super.init(drawingView: drawingView, container: drawingView)
        isAccessibilityElement = false
        accessibilityContainerType = .dataTable
        accessibilityLabel = TableAccessibilityText.table.localized
    }

    func refresh(_ table: TableAccessibilityTable) {
        self.table = table
        materializedCellElements.forEach { $1.refresh(table.cells[$0], tableID: table.identity) }
    }

    var materializedElements: [TableAccessibilityCellElement] {
        Array(materializedCellElements.values)
    }

    func cellElement(at index: Int) -> TableAccessibilityCellElement? {
        guard table.cells.indices.contains(index), let drawingView else { return nil }
        if let existing = materializedCellElements[index] { return existing }
        let element = TableAccessibilityCellElement(drawingView: drawingView, tableElement: self,
                                                    cell: table.cells[index], tableID: table.identity)
        materializedCellElements[index] = element
        return element
    }

    func cellElement(sourceIndex: Int) -> TableAccessibilityCellElement? {
        table.cellIndex(sourceIndex: sourceIndex).flatMap(cellElement(at:))
    }

    override func accessibilityElementCount() -> Int { table.cells.count }

    override func accessibilityElement(at index: Int) -> Any? {
        cellElement(at: index).map(element(for:))
    }

    override func index(ofAccessibilityElement element: Any) -> Int {
        let position: Int?
        switch element {
        case let cellElement as TableAccessibilityCellElement: position = cellElement.cell.sourceIndex
        case let input as TableCellInputTextView: position = input.tableAccessibilityCell?.cell()?.sourceIndex
        default: position = nil
        }
        return position.flatMap(table.cellIndex(sourceIndex:)) ?? NSNotFound
    }

    override var accessibilityFrame: CGRect {
        get {
            guard isCurrent, let presented = drawingView?.presentedRootTable(table.surface) else { return .zero }
            return screenFrame(presented.bounds, clip: presented.clip)
        }
        set { }
    }

    private func element(for cellElement: TableAccessibilityCellElement) -> UIAccessibilityContainerDataTableCell {
        guard isCurrent,
              let input = drawingView?.tableAccessibilityEditing?.activeTableAccessibilityElement(
                for: cellElement.cell, tableID: table.identity
              )
        else { return cellElement }
        return input
    }

    private func elements(_ cells: [TableAccessibilityCell]) -> [UIAccessibilityContainerDataTableCell] {
        cells.compactMap { cellElement(sourceIndex: $0.sourceIndex).map(element(for:)) }
    }

    func accessibilityDataTableCellElement(forRow row: Int, column: Int) -> UIAccessibilityContainerDataTableCell? {
        table.cellIndex(row: row, column: column).flatMap(cellElement(at:)).map(element(for:))
    }

    func accessibilityRowCount() -> Int { table.rowCount }

    func accessibilityColumnCount() -> Int { table.columnCount }

    func accessibilityHeaderElements(forRow row: Int) -> [UIAccessibilityContainerDataTableCell]? {
        elements(table.rowHeaders(row))
    }

    func accessibilityHeaderElements(forColumn column: Int) -> [UIAccessibilityContainerDataTableCell]? {
        elements(table.columnHeaders(column))
    }
}

final class TableAccessibilityCellElement: TableAccessibilityGeneratedElement, UIAccessibilityContainerDataTableCell {
    private weak var tableElement: TableAccessibilityTableElement?
    private(set) var cell: TableAccessibilityCell
    private(set) var tableID: String

    init(drawingView: PreparedProseDrawingView, tableElement: TableAccessibilityTableElement,
         cell: TableAccessibilityCell, tableID: String) {
        self.tableElement = tableElement
        self.cell = cell
        self.tableID = tableID
        super.init(drawingView: drawingView, container: tableElement)
        isAccessibilityElement = true
        refresh(cell, tableID: tableID)
    }

    func refresh(_ cell: TableAccessibilityCell, tableID: String) {
        self.cell = cell
        self.tableID = tableID
        accessibilityTraits = cell.isHeader ? [.staticText, .header] : .staticText
    }

    override var isCurrent: Bool { tableElement?.isCurrent == true }

    private var editing: TableAccessibilityEditing? { isCurrent ? drawingView?.tableAccessibilityEditing : nil }

    private var interactions: [ViewerTablePresentedAccessibilityNode] {
        drawingView?.tableAccessibilityInteractions(for: cell) ?? []
    }

    override var accessibilityLabel: String? {
        get {
            drawingView?.reconcileAccessibilityElementsIfNeeded()
            return cell.label.isEmpty ? TableAccessibilityText.emptyCell.localized : cell.label
        }
        set { }
    }

    override var accessibilityValue: String? {
        get {
            drawingView?.reconcileAccessibilityElementsIfNeeded()
            return cell.spanDescription
        }
        set { }
    }

    override var accessibilityFrame: CGRect {
        get {
            guard isCurrent, let presented = drawingView?.accessibilityCellGeometry(cell) else { return .zero }
            let visible = screenFrame(presented.bounds, clip: presented.clip)
            return visible.isEmpty ? screenFrame(presented.bounds, clip: .infinite) : visible
        }
        set { }
    }

    override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get {
            guard isCurrent else { return [] }
            let interactions = interactions.map { node in
                let name = node.node.role == .mention
                    ? TableAccessibilityText.openMention(node.node.label).localized
                    : TableAccessibilityText.openLink(node.node.label).localized
                let identity = node.sourceIdentity
                return UIAccessibilityCustomAction(name: name) { [weak self] _ in
                    self?.activateInteraction(identity) ?? false
                }
            }
            return interactions + TableAccessibility.customActions(for: cell, tableID: tableID, editing: editing)
        }
        set { }
    }

    private func activateInteraction(_ identity: String) -> Bool {
        guard isCurrent, let drawingView,
              let node = interactions.first(where: { $0.sourceIdentity == identity })
        else { return false }
        return drawingView.activateAccessibilityNode(node)
    }

    override func accessibilityActivate() -> Bool {
        editing?.activateTableAccessibilityCell(cell, tableID: tableID) ?? false
    }

    override func accessibilityElementDidBecomeFocused() {
        guard isCurrent else { return }
        drawingView?.revealTableAccessibilityCell(cell)
    }

    func accessibilityRowRange() -> NSRange { cell.rows }

    func accessibilityColumnRange() -> NSRange { cell.columns }
}

final class TableAccessibilityFrameElement: TableAccessibilityGeneratedElement {
    enum Source {
        case drawn(TableAccessibilityTable)
        case detached(TableAccessibilityDetachedFrame)
    }

    private(set) var source: Source

    init(drawingView: PreparedProseDrawingView, source: Source) {
        self.source = source
        super.init(drawingView: drawingView, container: drawingView)
        isAccessibilityElement = true
        accessibilityTraits = .staticText
        refresh(source)
    }

    var tableID: String {
        switch source {
        case let .drawn(table): return table.identity
        case let .detached(frame): return frame.tableID
        }
    }

    func refresh(_ source: Source) {
        self.source = source
        let frame: TableAccessibilityTable.Frame?
        switch source {
        case let .drawn(table): frame = table.frame
        case let .detached(detached): frame = detached.frame
        }
        accessibilityLabel = frame == .empty
            ? TableAccessibilityText.emptyTable.localized
            : TableAccessibilityText.failedTable.localized
    }

    override var accessibilityFrame: CGRect {
        get {
            guard isCurrent else { return .zero }
            switch source {
            case let .drawn(table):
                guard let presented = drawingView?.presentedRootTable(table.surface) else { return .zero }
                return screenFrame(presented.bounds, clip: presented.clip)
            case let .detached(frame): return frame.screenFrame()
            }
        }
        set { }
    }

    override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get {
            let tableID = tableID
            guard isCurrent, let editing = drawingView?.tableAccessibilityEditing,
                  editing.canDeleteTableAccessibilityFrame(tableID: tableID)
            else { return [] }
            return [UIAccessibilityCustomAction(name: TableAccessibilityAction.deleteTable.label) { [weak editing] _ in
                editing?.deleteTableAccessibilityFrame(tableID: tableID) ?? false
            }]
        }
        set { }
    }
}
