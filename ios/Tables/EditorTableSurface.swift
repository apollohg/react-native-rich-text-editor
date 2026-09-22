import UIKit

final class EditorTableSurface: UIView {
    let inputCoordinator: EditorTableInputCoordinator

    init(inputCoordinator: EditorTableInputCoordinator) {
        self.inputCoordinator = inputCoordinator
        super.init(frame: .zero)
        clipsToBounds = true
        inputCoordinator.cellInput.isHidden = true
        addSubview(inputCoordinator.cellInput)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func placeActiveInput(in contentRect: CGRect) {
        inputCoordinator.cellInput.frame = contentRect.integral
        inputCoordinator.cellInput.isHidden = false
    }

    func hideActiveInput() {
        inputCoordinator.cellInput.isHidden = true
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        guard !inputCoordinator.cellInput.isHidden,
              inputCoordinator.cellInput.frame.contains(point)
        else { return nil }
        return super.hitTest(point, with: event)
    }
}
