import UIKit

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
    let presented: ViewerTablePresentedCell
    let sourceCellIndex: Int
    let rows: NSRange
    let columns: NSRange
    let isHeader: Bool
    let label: String
    let interactions: [ViewerTablePresentedAccessibilityNode]

    var sourcePosition: Int { presented.sourcePosition }

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

    let presented: ViewerTablePresentedTable
    let rowCount: Int
    let columnCount: Int
    let cells: [TableAccessibilityCell]

    var identity: String { presented.surface.identity }
    var tablePos: UInt32? { presented.surface.sourceTable?.tablePos }

    var frame: Frame? {
        guard cells.isEmpty else { return nil }
        let unfilled = presented.surface.sourceTable?.failure == nil && (rowCount == 0 || columnCount == 0)
        return unfilled ? .empty : .failed
    }

    func cell(row: Int, column: Int) -> TableAccessibilityCell? {
        cells.first { NSLocationInRange(row, $0.rows) && NSLocationInRange(column, $0.columns) }
    }

    func columnHeaders(_ column: Int) -> [TableAccessibilityCell] {
        cells.filter { $0.isHeader && NSLocationInRange(column, $0.columns) && isHeaderRow($0.rows.location) }
    }

    func rowHeaders(_ row: Int) -> [TableAccessibilityCell] {
        cells.filter { $0.isHeader && NSLocationInRange(row, $0.rows) && isHeaderColumn($0.columns.location) }
    }

    private func isHeaderRow(_ row: Int) -> Bool {
        let starting = cells.filter { $0.rows.location == row }
        return !starting.isEmpty && starting.allSatisfy(\.isHeader)
    }

    private func isHeaderColumn(_ column: Int) -> Bool {
        let starting = cells.filter { $0.columns.location == column }
        return !starting.isEmpty && starting.allSatisfy(\.isHeader)
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
        snapshot: ViewerTablePresentationSnapshot,
        root: PreparedProseLayout,
        nodes: [ViewerTablePresentedAccessibilityNode],
        detachedFrames: [TableAccessibilityDetachedFrame]
    ) -> [TableAccessibilityItem] {
        var pendingFrames = detachedFrames.sorted { $0.tablePos < $1.tablePos }
        var merged: [TableAccessibilityItem] = []
        func flushFrames(before tablePos: UInt32?) {
            while let next = pendingFrames.first, tablePos.map({ next.tablePos < $0 }) ?? true {
                merged.append(.detachedFrame(next))
                pendingFrames.removeFirst()
            }
        }
        for item in drawnItems(snapshot: snapshot, root: root, nodes: nodes) {
            if case let .table(table) = item { flushFrames(before: table.tablePos ?? UInt32.max) }
            merged.append(item)
        }
        flushFrames(before: nil)
        return merged
    }

    private static func drawnItems(
        snapshot: ViewerTablePresentationSnapshot,
        root: PreparedProseLayout,
        nodes: [ViewerTablePresentedAccessibilityNode]
    ) -> [TableAccessibilityItem] {
        let rootTables = snapshot.tables.filter { $0.parentScrollIdentity == nil }
        let rootCells = rootTables.map { table in snapshot.cells.filter { $0.surface === table.surface } }
        var owners: [ObjectIdentifier: (table: Int, cell: Int)] = [:]
        func claim(_ layout: PreparedProseLayout, owner: (table: Int, cell: Int)) {
            owners[ObjectIdentifier(layout)] = owner
            for block in layout.blocks {
                block.tableSurface?.cells.forEach { claim($0.content, owner: owner) }
            }
        }
        for (tableIndex, cells) in rootCells.enumerated() {
            for (cellIndex, cell) in cells.enumerated() {
                claim(cell.content, owner: (tableIndex, cellIndex))
            }
        }
        var rootNodes: [ViewerTablePresentedAccessibilityNode] = []
        var cellNodes = rootCells.map { cells in cells.map { _ in [ViewerTablePresentedAccessibilityNode]() } }
        for node in nodes {
            if node.layout === root {
                rootNodes.append(node)
            } else if let owner = owners[ObjectIdentifier(node.layout)] {
                cellNodes[owner.table][owner.cell].append(node)
            }
        }
        let tables = rootTables.enumerated().map { index, presented in
            table(presented, cells: rootCells[index], nodes: cellNodes[index])
        }
        let blockIndexes = rootTables.map { presented in
            root.blocks.firstIndex { $0.tableSurface === presented.surface } ?? root.blocks.count
        }
        var pending = Array(zip(blockIndexes, tables))
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

    private static func table(
        _ presented: ViewerTablePresentedTable,
        cells: [ViewerTablePresentedCell],
        nodes: [[ViewerTablePresentedAccessibilityNode]]
    ) -> TableAccessibilityTable {
        let source = presented.surface.sourceTable
        let accessibleCells = zip(cells, nodes).compactMap { cell, nodes -> TableAccessibilityCell? in
            guard let index = cell.cell.sourceCellIndex,
                  let sourceCell = source?.cells[index]
            else { return nil }
            return TableAccessibilityCell(
                presented: cell,
                sourceCellIndex: index,
                rows: NSRange(location: Int(sourceCell.row), length: Int(sourceCell.rowspan)),
                columns: NSRange(location: Int(sourceCell.column), length: Int(sourceCell.colspan)),
                isHeader: sourceCell.header,
                label: nodes.map(\.node.label).filter { !$0.isEmpty }.joined(separator: labelSeparator),
                interactions: nodes.filter { $0.node.interactionIndex != nil }
            )
        }
        return TableAccessibilityTable(
            presented: presented,
            rowCount: source.map { Int($0.rows) } ?? max(0, presented.surface.layout.rowOffsets.count - 1),
            columnCount: source.map { Int($0.columns) } ?? presented.surface.layout.columnWidths.count,
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
    private(set) var cellElements: [TableAccessibilityCellElement] = []

    init(drawingView: PreparedProseDrawingView, table: TableAccessibilityTable) {
        self.table = table
        super.init(drawingView: drawingView, container: drawingView)
        isAccessibilityElement = false
        accessibilityContainerType = .dataTable
        accessibilityLabel = TableAccessibilityText.table.localized
        cellElements = table.cells.map {
            TableAccessibilityCellElement(drawingView: drawingView, tableElement: self, cell: $0, tableID: table.identity)
        }
    }

    func refresh(_ table: TableAccessibilityTable) {
        self.table = table
        zip(cellElements, table.cells).forEach { $0.refresh($1, tableID: table.identity) }
    }

    override var accessibilityElements: [Any]? {
        get { cellElements.map(element(for:)) }
        set { }
    }

    override var accessibilityFrame: CGRect {
        get {
            guard isCurrent else { return .zero }
            return screenFrame(table.presented.bounds, clip: table.presented.clip)
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
        cells.compactMap { cell in
            cellElements.first { $0.cell.sourcePosition == cell.sourcePosition }.map(element(for:))
        }
    }

    func accessibilityDataTableCellElement(forRow row: Int, column: Int) -> UIAccessibilityContainerDataTableCell? {
        table.cell(row: row, column: column).flatMap { elements([$0]).first }
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

    var linkNodes: [ViewerTablePresentedAccessibilityNode] {
        cell.interactions.filter { $0.node.role == .link }
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
            guard isCurrent else { return .zero }
            let visible = screenFrame(cell.presented.bounds, clip: cell.presented.clip)
            return visible.isEmpty ? screenFrame(cell.presented.bounds, clip: .infinite) : visible
        }
        set { }
    }

    override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get {
            guard isCurrent else { return [] }
            let interactions = cell.interactions.map { node in
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
              let node = cell.interactions.first(where: { $0.sourceIdentity == identity })
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
            case let .drawn(table): return screenFrame(table.presented.bounds, clip: table.presented.clip)
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
