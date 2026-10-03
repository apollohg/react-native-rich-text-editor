import UIKit

final class TableHorizontalPanGestureRecognizer: UIPanGestureRecognizer {
    private enum Constants {
        static let intentSlop: CGFloat = 8
    }

    private var initialLocation: CGPoint?
    var claimsDirection: ((CGFloat) -> Bool)?

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard event.allTouches?.count == 1, let touch = touches.first else {
            state = state == .possible ? .failed : .cancelled
            return
        }
        if state == .possible { initialLocation = touch.location(in: view) }
        super.touchesBegan(touches, with: event)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        if state == .possible, let initialLocation, let touch = touches.first {
            let location = touch.location(in: view)
            let delta = CGPoint(x: location.x - initialLocation.x, y: location.y - initialLocation.y)
            guard claimsMovement(by: delta) else {
                state = .failed
                return
            }
        }
        super.touchesMoved(touches, with: event)
    }

    func claimsMovement(by delta: CGPoint) -> Bool {
        guard hypot(delta.x, delta.y) >= Constants.intentSlop else { return true }
        return TableInteractionController.isHorizontalIntent(delta) && claimsDirection?(delta.x) != false
    }

    override func reset() {
        super.reset()
        initialLocation = nil
    }
}

final class TableInteractionController: NSObject, UIGestureRecognizerDelegate {
    private final class WeakScrollView {
        weak var value: UIScrollView?

        init(_ value: UIScrollView) {
            self.value = value
        }
    }

    private enum Constants {
        static let horizontalRatio: CGFloat = 1.25
        static let scrollDirections: [CGFloat] = [-1, 1]
        static let minimumVelocity: CGFloat = 5
        static let millisecondsPerSecond: Double = 1_000
    }

    private weak var host: UIView?
    private weak var drawing: PreparedProseDrawingView?
    private let pan = TableHorizontalPanGestureRecognizer()
    private var touchPoint = CGPoint.zero
    private weak var touchedView: UIView?
    private var chain: [String] = []
    private var previousTranslation: CGFloat = 0
    private var outerScrollViews: [WeakScrollView] = []
    private var displayLink: CADisplayLink?
    private var velocity: CGFloat = 0
    private var lastTimestamp: CFTimeInterval = 0
    private var motionGeneration: UInt64 = 0

    init(host: UIView, drawing: PreparedProseDrawingView) {
        self.host = host
        self.drawing = drawing
        super.init()
        pan.addTarget(self, action: #selector(handlePan(_:)))
        pan.maximumNumberOfTouches = 1
        pan.delegate = self
        pan.claimsDirection = { [weak self] delta in self?.claimsTouch(towards: delta) ?? false }
        host.addGestureRecognizer(pan)
    }

    static func isHorizontalIntent(_ translation: CGPoint) -> Bool {
        abs(translation.x) > Constants.horizontalRatio * abs(translation.y)
    }

    func detach() {
        cancelMotion()
        pan.view?.removeGestureRecognizer(pan)
    }

    func cancelMotion() {
        motionGeneration &+= 1
        displayLink?.invalidate()
        displayLink = nil
        velocity = 0
        chain = []
        outerScrollViews = []
        previousTranslation = 0
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard gestureRecognizer === pan, touch.tapCount == 1,
              let drawing, let host, drawing.window != nil,
              touch.view?.window === drawing.window
        else { return false }
        cancelMotion()
        let point = touch.location(in: drawing)
        guard !drawing.tableChain(at: point).isEmpty else { return false }
        if drawing.mountedTablePresentation()?.atoms.contains(where: {
            $0.bounds.contains(point) && $0.clip.contains(point)
        }) == true, hasOwnedInnerGesture(from: touch.view, through: host) {
            return false
        }
        touchPoint = point
        touchedView = touch.view
        return true
    }

    func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        guard gestureRecognizer === pan, let host, let drawing, drawing.window != nil,
              !nativeSelectionGestureIsActive()
        else { return false }
        let selectedChain = drawing.tableChain(at: touchPoint)
        guard !selectedChain.isEmpty else { return false }
        let outer = horizontalScrollAncestors(of: host)
        guard Constants.scrollDirections.contains(where: { direction in
            canScroll(selectedChain, outer, by: direction)
        }) else { return false }
        cancelMotion()
        chain = selectedChain
        outerScrollViews = outer.map(WeakScrollView.init)
        previousTranslation = 0
        return true
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldBeRequiredToFailBy otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        guard gestureRecognizer === pan, !drawingChainAtTouch().isEmpty,
              let scrollView = otherGestureRecognizer.view as? UIScrollView,
              otherGestureRecognizer === scrollView.panGestureRecognizer
        else { return false }
        guard let host else { return false }
        return scrollView === host || scrollView.isDescendant(of: host)
            || host.isDescendant(of: scrollView)
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRequireFailureOf otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === pan && otherGestureRecognizer is UIScreenEdgePanGestureRecognizer
    }

    @objc private func handlePan(_ recognizer: UIPanGestureRecognizer) {
        guard recognizer === pan, let host, let drawing, drawing.window != nil,
              !chain.isEmpty, !nativeSelectionGestureIsActive()
        else {
            cancelMotion()
            return
        }
        switch recognizer.state {
        case .began, .changed, .ended:
            let generation = motionGeneration
            let translation = recognizer.translation(in: host).x
            let delta = translation - previousTranslation
            previousTranslation = translation
            consume(delta)
            guard generation == motionGeneration else { return }
            if recognizer.state == .ended {
                velocity = recognizer.velocity(in: host).x
                startDecelerationIfNeeded()
            }
        case .cancelled, .failed:
            cancelMotion()
        default:
            break
        }
    }

    private func claimsTouch(towards delta: CGFloat) -> Bool {
        guard let host else { return false }
        return canScroll(drawingChainAtTouch(), horizontalScrollAncestors(of: host), by: delta)
    }

    private func canScroll(_ chain: [String], _ outer: [UIScrollView], by delta: CGFloat) -> Bool {
        guard let drawing else { return false }
        return drawing.canScrollTables(in: chain, by: delta) || outer.contains { canScroll($0, by: delta) }
    }

    private func drawingChainAtTouch() -> [String] {
        drawing?.tableChain(at: touchPoint) ?? []
    }

    private func nativeSelectionGestureIsActive() -> Bool {
        var view = touchedView
        while let current = view {
            for recognizer in current.gestureRecognizers ?? [] where recognizer !== pan {
                if let scroll = current as? UIScrollView, recognizer === scroll.panGestureRecognizer {
                    continue
                }
                if recognizer.state == .began || recognizer.state == .changed {
                    return true
                }
            }
            if current === host { break }
            view = current.superview
        }
        return false
    }

    private func hasOwnedInnerGesture(from touchedView: UIView?, through host: UIView) -> Bool {
        var view = touchedView
        while let current = view, current !== host {
            for recognizer in current.gestureRecognizers ?? [] {
                if let scroll = current as? UIScrollView, recognizer === scroll.panGestureRecognizer {
                    continue
                }
                if recognizer is UIPanGestureRecognizer || recognizer is UILongPressGestureRecognizer {
                    return true
                }
            }
            view = current.superview
        }
        return false
    }

    private func horizontalScrollAncestors(of view: UIView) -> [UIScrollView] {
        var result: [UIScrollView] = []
        var ancestor: UIView? = view
        while let current = ancestor {
            if let scroll = current as? UIScrollView, scroll.isScrollEnabled,
               scroll.contentSize.width + scroll.adjustedContentInset.left + scroll.adjustedContentInset.right > scroll.bounds.width {
                result.append(scroll)
            }
            ancestor = current.superview
        }
        return result
    }

    private func canScroll(_ scrollView: UIScrollView?, by delta: CGFloat) -> Bool {
        guard let scrollView, scrollView.isScrollEnabled, delta != 0 else { return false }
        let minimum = -scrollView.adjustedContentInset.left
        let maximum = max(
            minimum,
            scrollView.contentSize.width - scrollView.bounds.width
                + scrollView.adjustedContentInset.right
        )
        return delta > 0 ? scrollView.contentOffset.x > minimum : scrollView.contentOffset.x < maximum
    }

    private func consume(_ delta: CGFloat) {
        guard let drawing, delta.isFinite, delta != 0 else { return }
        let generation = motionGeneration
        let remainder = drawing.scrollTables(in: chain, by: delta)
        guard generation == motionGeneration else { return }
        guard remainder != 0 else { return }
        var remaining = remainder
        for outer in outerScrollViews where remaining != 0 {
            guard let scroll = outer.value, scroll.isScrollEnabled else { continue }
            let minimum = -scroll.adjustedContentInset.left
            let maximum = max(
                minimum,
                scroll.contentSize.width - scroll.bounds.width
                    + scroll.adjustedContentInset.right
            )
            let old = scroll.contentOffset.x
            let next = min(maximum, max(minimum, old - remaining))
            if next != old {
                scroll.setContentOffset(CGPoint(x: next, y: scroll.contentOffset.y), animated: false)
                guard generation == motionGeneration else { return }
                remaining -= old - scroll.contentOffset.x
            }
        }
    }

    private func startDecelerationIfNeeded() {
        guard abs(velocity) >= Constants.minimumVelocity else {
            cancelMotion()
            return
        }
        lastTimestamp = 0
        let link = CADisplayLink(target: self, selector: #selector(stepDeceleration(_:)))
        displayLink = link
        link.add(to: .main, forMode: .common)
    }

    @objc private func stepDeceleration(_ link: CADisplayLink) {
        guard link === displayLink,
              let drawing, drawing.window != nil, host?.window === drawing.window,
              !chain.isEmpty else {
            if link === displayLink { cancelMotion() }
            return
        }
        if lastTimestamp == 0 {
            lastTimestamp = link.timestamp
            return
        }
        let elapsed = link.timestamp - lastTimestamp
        lastTimestamp = link.timestamp
        guard elapsed > 0, elapsed.isFinite else { return }
        let delta = velocity * elapsed
        guard canScroll(chain, outerScrollViews.compactMap(\.value), by: delta) else {
            cancelMotion()
            return
        }
        let generation = motionGeneration
        consume(delta)
        guard generation == motionGeneration else { return }
        let rate = Double(outerScrollViews.compactMap(\.value).first?.decelerationRate.rawValue
            ?? UIScrollView.DecelerationRate.normal.rawValue)
        velocity *= CGFloat(pow(rate, elapsed * Constants.millisecondsPerSecond))
        if abs(velocity) < Constants.minimumVelocity { cancelMotion() }
    }
}
