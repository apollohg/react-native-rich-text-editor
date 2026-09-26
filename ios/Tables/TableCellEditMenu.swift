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
    private var visibleIdentifier: NSString?
    private var dismissedAt = -TimeInterval.infinity

    var isVisible: Bool { visibleIdentifier != nil }

    init(anchor: @escaping () -> CGRect?, visibilityChanged: @escaping () -> Void) {
        self.anchor = anchor
        self.visibilityChanged = visibilityChanged
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
        return commands.isEmpty ? nil : UIMenu(children: commands)
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
