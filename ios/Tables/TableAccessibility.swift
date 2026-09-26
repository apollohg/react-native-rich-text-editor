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

enum TableAccessibilityItem {
    case node(ViewerTablePresentedAccessibilityNode)
    case table(TableAccessibilityTable)
}

enum TableAccessibility {
    static let labelSeparator = " "
    static let descriptionSeparator = ", "

    static func items(
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
    func activeTableAccessibilityElement(for cell: TableAccessibilityCell, tableID: String) -> EditorTextView?
    func canDeleteTableAccessibilityFrame(tableID: String) -> Bool
    func deleteTableAccessibilityFrame(tableID: String) -> Bool
}

struct TableAccessibilityActiveCell {
    let rows: NSRange
    let columns: NSRange
    let actions: () -> [UIAccessibilityCustomAction]
}

extension EditorTextView: UIAccessibilityContainerDataTableCell {
    func accessibilityRowRange() -> NSRange {
        tableAccessibilityCell?.rows ?? NSRange(location: NSNotFound, length: 0)
    }

    func accessibilityColumnRange() -> NSRange {
        tableAccessibilityCell?.columns ?? NSRange(location: NSNotFound, length: 0)
    }
}

class TableAccessibilityGeneratedElement: UIAccessibilityElement {
    weak var drawingView: PreparedProseDrawingView?
    let generation: Int

    init(drawingView: PreparedProseDrawingView, container: Any, generation: Int) {
        self.drawingView = drawingView
        self.generation = generation
        super.init(accessibilityContainer: container)
    }

    var isCurrent: Bool { drawingView?.isCurrentAccessibilityGeneration(generation) == true }

    func screenFrame(_ rect: CGRect, clip: CGRect) -> CGRect {
        guard isCurrent, let drawingView else { return .zero }
        return drawingView.accessibilityScreenFrame(rect, clip: clip)
    }
}

final class TableAccessibilityTableElement: TableAccessibilityGeneratedElement, UIAccessibilityContainerDataTable {
    let table: TableAccessibilityTable
    private(set) var cellElements: [TableAccessibilityCellElement] = []

    init(drawingView: PreparedProseDrawingView, generation: Int, table: TableAccessibilityTable) {
        self.table = table
        super.init(drawingView: drawingView, container: drawingView, generation: generation)
        isAccessibilityElement = false
        accessibilityContainerType = .dataTable
        accessibilityLabel = TableAccessibilityText.table.localized
        cellElements = table.cells.map {
            TableAccessibilityCellElement(drawingView: drawingView, container: self, generation: generation,
                                          cell: $0, tableID: table.identity)
        }
    }

    override var accessibilityElements: [Any]? {
        get { cellElements.map(element(for:)) }
        set { }
    }

    override var accessibilityFrame: CGRect {
        get { screenFrame(table.presented.bounds, clip: table.presented.clip) }
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
    let cell: TableAccessibilityCell
    let tableID: String

    init(drawingView: PreparedProseDrawingView, container: Any, generation: Int,
         cell: TableAccessibilityCell, tableID: String) {
        self.cell = cell
        self.tableID = tableID
        super.init(drawingView: drawingView, container: container, generation: generation)
        isAccessibilityElement = true
        accessibilityLabel = cell.label.isEmpty ? TableAccessibilityText.emptyCell.localized : cell.label
        accessibilityValue = cell.spanDescription
        accessibilityTraits = cell.isHeader ? [.staticText, .header] : .staticText
    }

    private var editing: TableAccessibilityEditing? { isCurrent ? drawingView?.tableAccessibilityEditing : nil }

    var linkNodes: [ViewerTablePresentedAccessibilityNode] {
        cell.interactions.filter { $0.node.role == .link }
    }

    override var accessibilityFrame: CGRect {
        get {
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
                return UIAccessibilityCustomAction(name: name) { [weak self] _ in
                    self?.activate(node) ?? false
                }
            }
            return interactions + TableAccessibility.customActions(for: cell, tableID: tableID, editing: editing)
        }
        set { }
    }

    private func activate(_ node: ViewerTablePresentedAccessibilityNode) -> Bool {
        guard isCurrent, let drawingView else { return false }
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

final class TableAccessibilityFrameElement: UIAccessibilityElement {
    let tableID: String
    private let editing: () -> TableAccessibilityEditing?
    private let screenFrame: () -> CGRect

    init(container: Any, tableID: String, frame: TableAccessibilityTable.Frame,
         editing: @escaping () -> TableAccessibilityEditing?, screenFrame: @escaping () -> CGRect) {
        self.tableID = tableID
        self.editing = editing
        self.screenFrame = screenFrame
        super.init(accessibilityContainer: container)
        isAccessibilityElement = true
        accessibilityLabel = frame == .empty
            ? TableAccessibilityText.emptyTable.localized
            : TableAccessibilityText.failedTable.localized
        accessibilityTraits = .staticText
    }

    override var accessibilityFrame: CGRect {
        get { screenFrame() }
        set { }
    }

    override var accessibilityCustomActions: [UIAccessibilityCustomAction]? {
        get {
            guard let editing = editing(), editing.canDeleteTableAccessibilityFrame(tableID: tableID) else { return [] }
            let tableID = tableID
            return [UIAccessibilityCustomAction(name: TableAccessibilityAction.deleteTable.label) { [weak editing] _ in
                editing?.deleteTableAccessibilityFrame(tableID: tableID) ?? false
            }]
        }
        set { }
    }
}
