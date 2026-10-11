import UIKit

extension UIGestureRecognizer {
    func isContentPopGesture(above view: UIView) -> Bool {
        #if compiler(>=6.2)
            if #available(iOS 26.0, *) {
                var responder: UIResponder? = view
                while let current = responder {
                    if let navigation = current as? UINavigationController,
                       navigation.interactiveContentPopGestureRecognizer === self {
                        return true
                    }
                    responder = current.next
                }
            }
        #endif
        return false
    }

    func isEdgePopGesture(above view: UIView) -> Bool {
        self is UIScreenEdgePanGestureRecognizer && !isContentPopGesture(above: view)
    }
}

final class TableHorizontalPanGestureRecognizer: UIPanGestureRecognizer {
    private enum Constants {
        static let intentSlop: CGFloat = 8
    }

    private var initialLocation: CGPoint?

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
        return TableInteractionController.isHorizontalIntent(delta)
    }

    override func reset() {
        super.reset()
        initialLocation = nil
    }
}

final class TableInteractionController: UIScrollView, UIScrollViewDelegate, UIGestureRecognizerDelegate {
    private final class WeakScrollView {
        weak var value: UIScrollView?

        init(_ value: UIScrollView) {
            self.value = value
        }
    }

    private enum Constants {
        static let horizontalRatio: CGFloat = 1.25
    }

    private weak var host: UIView?
    private weak var drawing: PreparedProseDrawingView?
    private weak var touchedView: UIView?
    private var chain: [String] = []
    private var outerScrollViews: [WeakScrollView] = []
    private var lastClampedOffset: CGFloat = 0
    private var maximumOffset: CGFloat = 0
    private var overscrollIdentity: String?
    private var isConfiguringMotion = false
    private var motionGeneration: UInt64 = 0

    init(host: UIView, drawing: PreparedProseDrawingView) {
        self.host = host
        self.drawing = drawing
        super.init(frame: host.bounds)
        autoresizingMask = [.flexibleWidth, .flexibleHeight]
        backgroundColor = .clear
        isAccessibilityElement = false
        accessibilityElementsHidden = true
        contentInsetAdjustmentBehavior = .never
        showsHorizontalScrollIndicator = false
        showsVerticalScrollIndicator = false
        scrollsToTop = false
        delaysContentTouches = false
        isDirectionalLockEnabled = true
        bounces = true
        delegate = self
        panGestureRecognizer.maximumNumberOfTouches = 1
        panGestureRecognizer.delegate = self
        host.addSubview(self)
        // UIKit owns drag, deceleration and spring-back; the renderer owns table positions.
        host.addGestureRecognizer(panGestureRecognizer)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("TableInteractionController does not support NSCoder") }

    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool { false }

    static func isHorizontalIntent(_ translation: CGPoint) -> Bool {
        abs(translation.x) > Constants.horizontalRatio * abs(translation.y)
    }

    func detach() {
        cancelMotion()
        panGestureRecognizer.view?.removeGestureRecognizer(panGestureRecognizer)
        delegate = nil
        removeFromSuperview()
    }

    func cancelMotion() {
        motionGeneration &+= 1
        let previousChain = chain
        let previousOverscroll = overscrollIdentity
        let previousOuter = outerScrollViews.compactMap(\.value)
        chain = []
        overscrollIdentity = nil
        outerScrollViews = []
        isConfiguringMotion = true
        if isDragging || isDecelerating {
            isScrollEnabled = false
            isScrollEnabled = true
        }
        setContentOffset(.zero, animated: false)
        lastClampedOffset = 0
        maximumOffset = 0
        isConfiguringMotion = false
        if let previousOverscroll {
            drawing?.scrollTables(in: previousChain, by: 0, overscrollIdentity: previousOverscroll)
        }
        for scroll in previousOuter {
            let limits = horizontalLimits(of: scroll)
            let clamped = min(limits.maximum, max(limits.minimum, scroll.contentOffset.x))
            if clamped != scroll.contentOffset.x {
                scroll.setContentOffset(CGPoint(x: clamped, y: scroll.contentOffset.y), animated: false)
            }
        }
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        guard gestureRecognizer === panGestureRecognizer, touch.tapCount == 1,
              let drawing, let host, drawing.window != nil,
              touch.view?.window === drawing.window
        else { return false }
        cancelMotion()
        let point = touch.location(in: drawing)
        let selectedChain = drawing.tableChain(at: point)
        guard !selectedChain.isEmpty else { return false }
        if drawing.mountedTablePresentation()?.atoms.contains(where: {
            $0.bounds.contains(point) && $0.clip.contains(point)
        }) == true, hasOwnedInnerGesture(from: touch.view, through: host) {
            return false
        }
        touchedView = touch.view
        let outer = horizontalScrollAncestors(of: host)
        var range = drawing.tableScrollRange(in: selectedChain)
        for scroll in outer {
            let limits = horizontalLimits(of: scroll)
            range.offset += min(limits.maximum, max(limits.minimum, scroll.contentOffset.x)) - limits.minimum
            range.maximum += limits.maximum - limits.minimum
        }
        guard range.maximum > 0 else { return false }
        chain = selectedChain
        outerScrollViews = outer.map(WeakScrollView.init)
        overscrollIdentity = outer.isEmpty ? selectedChain.last : nil
        maximumOffset = range.maximum
        lastClampedOffset = range.offset
        isConfiguringMotion = true
        contentSize = CGSize(width: bounds.width + maximumOffset, height: bounds.height)
        setContentOffset(CGPoint(x: range.offset, y: 0), animated: false)
        isConfiguringMotion = false
        return true
    }

    override func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        guard gestureRecognizer === panGestureRecognizer, let host, let drawing, drawing.window != nil,
              !chain.isEmpty, !nativeSelectionGestureIsActive()
        else { return false }
        let velocity = panGestureRecognizer.velocity(in: host)
        return Self.isHorizontalIntent(velocity)
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRequireFailureOf otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === panGestureRecognizer && otherGestureRecognizer.isEdgePopGesture(above: self)
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldBeRequiredToFailBy otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === panGestureRecognizer && otherGestureRecognizer.isContentPopGesture(above: self)
    }

    func scrollViewDidScroll(_ scrollView: UIScrollView) {
        guard !isConfiguringMotion, !chain.isEmpty, let drawing,
              drawing.window != nil, host?.window === drawing.window
        else { return }
        let offset = scrollView.contentOffset.x
        guard offset.isFinite else { return }
        let clamped = min(maximumOffset, max(0, offset))
        let delta = lastClampedOffset - clamped
        lastClampedOffset = clamped
        consume(delta, overscroll: offset - clamped)
    }

    private func nativeSelectionGestureIsActive() -> Bool {
        var view = touchedView
        while let current = view {
            for recognizer in current.gestureRecognizers ?? [] where recognizer !== panGestureRecognizer {
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
            if let scroll = current as? UIScrollView, scroll.isScrollEnabled {
                let limits = horizontalLimits(of: scroll)
                if limits.maximum > limits.minimum { result.append(scroll) }
            }
            ancestor = current.superview
        }
        return result
    }

    private func horizontalLimits(of scroll: UIScrollView) -> (minimum: CGFloat, maximum: CGFloat) {
        let minimum = -scroll.adjustedContentInset.left
        return (minimum, max(minimum, scroll.contentSize.width - scroll.bounds.width + scroll.adjustedContentInset.right))
    }

    private func consume(_ delta: CGFloat, overscroll: CGFloat) {
        guard let drawing, delta.isFinite, overscroll.isFinite else { return }
        let generation = motionGeneration
        var remaining = drawing.scrollTables(
            in: chain, by: delta, physicalOverscroll: overscroll,
            overscrollIdentity: overscrollIdentity
        )
        guard generation == motionGeneration else { return }
        let outer = outerScrollViews.compactMap(\.value).filter(\.isScrollEnabled)
        for (index, scroll) in outer.enumerated() {
            let limits = horizontalLimits(of: scroll)
            let old = min(limits.maximum, max(limits.minimum, scroll.contentOffset.x))
            let next = min(limits.maximum, max(limits.minimum, old - remaining))
            remaining -= old - next
            let elastic = index == outer.count - 1 && scroll.bounces ? overscroll : 0
            if scroll.contentOffset.x != next + elastic {
                scroll.setContentOffset(CGPoint(x: next + elastic, y: scroll.contentOffset.y), animated: false)
                guard generation == motionGeneration else { return }
            }
        }
    }
}

final class TableTextSelectionGestureRecognizer: UIGestureRecognizer {
    var crossesCellBoundary: ((CGPoint) -> Bool)?
    var trackingDidReset: (() -> Void)?
    private var primaryTouch: UITouch?

    override func location(in view: UIView?) -> CGPoint {
        primaryTouch?.location(in: view) ?? super.location(in: view)
    }

    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool {
        preventingGestureRecognizer is UIScreenEdgePanGestureRecognizer
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard primaryTouch == nil, touches.count == 1, event.allTouches?.count == 1 else {
            state = state == .possible ? .failed : .cancelled
            return
        }
        primaryTouch = touches.first
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let primaryTouch, touches.contains(primaryTouch), event.allTouches?.count == 1 else {
            state = state == .possible ? .failed : .cancelled
            return
        }
        if state == .possible {
            if crossesCellBoundary?(primaryTouch.location(in: view)) == true { state = .began }
        } else {
            state = .changed
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        state = state == .possible ? .failed : (primaryTouch.map(touches.contains) == true ? .ended : .cancelled)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        state = state == .possible ? .failed : .cancelled
    }

    override func reset() {
        super.reset()
        primaryTouch = nil
        trackingDidReset?()
    }
}
