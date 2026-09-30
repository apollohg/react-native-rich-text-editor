import UIKit
import XCTest

func makeTestWindow(frame: CGRect) -> UIWindow {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    precondition(scenes.count == 1, "NativeEditorTestHost must provide exactly one connected window scene")
    let window = UIWindow(windowScene: scenes[0])
    window.frame = frame
    return window
}

final class TableTestFrameClock: NSObject {
    struct Presentation {
        let start: Double
        let actionEnd: Double
        let commit: Double
        let displayed: Double
    }

    static let runLoopSlice = 0.005
    private static let frameTimeout = 120.0
    private var link: CADisplayLink!
    var onTick: ((CADisplayLink) -> Void)?

    override init() {
        super.init()
        link = CADisplayLink(target: self, selector: #selector(tick(_:)))
        PreparedProseInstrumentation.configureBenchmarkCadence(link)
        link.add(to: .main, forMode: .common)
    }

    func close() { onTick = nil; link.invalidate() }

    @objc private func tick(_ link: CADisplayLink) {
        onTick?(link)
    }

    func present(_ drawing: PreparedProseDrawingView, action: () throws -> Void = {}) throws -> Presentation {
        var commitTime: Double?
        var displayed: Double?
        drawing.onMountedTableCellsDrawnForTesting = { _ in
            CATransaction.setCompletionBlock { if commitTime == nil { commitTime = CACurrentMediaTime() } }
        }
        onTick = { link in
            guard displayed == nil, let commitTime, link.timestamp >= commitTime else { return }
            displayed = link.timestamp
        }
        defer {
            drawing.onMountedTableCellsDrawnForTesting = nil
            onTick = nil
        }
        let start = CACurrentMediaTime()
        try action()
        let actionEnd = CACurrentMediaTime()
        drawing.setNeedsDisplay()
        CATransaction.flush()
        let deadline = start + Self.frameTimeout
        while displayed == nil && CACurrentMediaTime() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(Self.runLoopSlice))
        }
        return Presentation(start: start, actionEnd: actionEnd,
            commit: try XCTUnwrap(commitTime, "No commit followed the dirty table transaction"),
            displayed: try XCTUnwrap(displayed, "No displayed frame followed the dirty table transaction"))
    }
}
