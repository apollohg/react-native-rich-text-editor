import UIKit

final class TableCellEditMenu: NSObject, UIEditMenuInteractionDelegate {
    static let actions = [
        #selector(UIResponderStandardEditActions.cut(_:)),
        #selector(UIResponderStandardEditActions.copy(_:)),
        #selector(UIResponderStandardEditActions.paste(_:))
    ]

    private(set) lazy var interaction = UIEditMenuInteraction(delegate: self)
    private let anchor: () -> CGRect?
    private let visibilityChanged: () -> Void
    private let tableActions: () -> [UIMenuElement]
    private var visibleIdentifier: NSString?
    private var dismissedAt = -TimeInterval.infinity

    var isVisible: Bool { visibleIdentifier != nil }

    init(
        anchor: @escaping () -> CGRect?,
        visibilityChanged: @escaping () -> Void,
        tableActions: @escaping () -> [UIMenuElement]
    ) {
        self.anchor = anchor
        self.visibilityChanged = visibilityChanged
        self.tableActions = tableActions
        super.init()
    }

    func present() {
        guard interaction.view?.window != nil, let rect = anchor() else { return }
        interaction.presentEditMenu(with: UIEditMenuConfiguration(
            identifier: UUID().uuidString as NSString,
            sourcePoint: CGPoint(x: rect.midX, y: rect.midY)
        ))
    }

    func dismiss() {
        guard isVisible else { return }
        interaction.dismissMenu()
        markDismissed()
    }

    func reanchor() {
        guard isVisible else { return }
        guard anchor() != nil else {
            dismiss()
            return
        }
        interaction.updateVisibleMenuPosition(animated: false)
    }

    func wasVisible(since timestamp: TimeInterval) -> Bool {
        isVisible || dismissedAt >= timestamp
    }

    static func commands(in suggestedActions: [UIMenuElement], performableBy responder: UIResponder) -> [UICommand] {
        let available = suggestedActions.flatMap(flattenedCommands)
        return actions.compactMap { action in
            available.first { $0.action == action && responder.canPerformAction(action, withSender: $0) }
        }
    }

    private static func flattenedCommands(_ element: UIMenuElement) -> [UICommand] {
        if let menu = element as? UIMenu { return menu.children.flatMap(flattenedCommands) }
        return (element as? UICommand).map { [$0] } ?? []
    }

    static func groupedActions(_ actions: [TableAccessibilityAction], perform: @escaping (TableAccessibilityAction) -> Void) -> [UIMenuElement] {
        func item(_ action: TableAccessibilityAction) -> UIAction {
            UIAction(
                title: action.label, identifier: UIAction.Identifier(action.key),
                attributes: action.isDestructive ? .destructive : []
            ) { _ in perform(action) }
        }
        var items: [UIMenuElement] = TableActionMenuGroup.allCases.filter { $0 != .more }.compactMap { group in
            let children = actions.filter { $0.menuGroup == group }.map(item)
            return children.isEmpty ? nil : UIMenu(title: group.label, children: children)
        }
        items.append(contentsOf: actions.filter { $0.menuGroup == nil }.map(item))
        let more = actions.filter { $0.menuGroup == .more }.map(item)
        if !more.isEmpty { items.append(UIMenu(title: TableActionMenuGroup.more.label, children: more)) }
        return items
    }

    private func markDismissed() {
        visibleIdentifier = nil
        dismissedAt = ProcessInfo.processInfo.systemUptime
        visibilityChanged()
    }

    func editMenuInteraction(
        _ interaction: UIEditMenuInteraction,
        menuFor configuration: UIEditMenuConfiguration,
        suggestedActions: [UIMenuElement]
    ) -> UIMenu? {
        guard let responder = interaction.view else { return nil }
        let commands = Self.commands(in: suggestedActions, performableBy: responder)
        let actions = tableActions()
        let clipboard: [UIMenuElement] = commands.isEmpty ? [] : [UIMenu(options: .displayInline, children: commands)]
        return UIMenu(children: clipboard + actions)
    }

    func editMenuInteraction(
        _ interaction: UIEditMenuInteraction,
        targetRectFor configuration: UIEditMenuConfiguration
    ) -> CGRect {
        anchor() ?? .null
    }

    func editMenuInteraction(
        _ interaction: UIEditMenuInteraction,
        willPresentMenuFor configuration: UIEditMenuConfiguration,
        animator: any UIEditMenuInteractionAnimating
    ) {
        visibleIdentifier = configuration.identifier as? NSString
        visibilityChanged()
    }

    func editMenuInteraction(
        _ interaction: UIEditMenuInteraction,
        willDismissMenuFor configuration: UIEditMenuConfiguration,
        animator: any UIEditMenuInteractionAnimating
    ) {
        guard let identifier = configuration.identifier as? NSString,
              identifier == visibleIdentifier
        else { return }
        markDismissed()
    }
}
