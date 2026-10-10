import UIKit
import XCTest

final class TouchObservingTestWindow: UIWindow {
    var onTouchHitTest: ((UIView) -> Void)?
    var onTouchEnded: ((UITouch) -> Void)?

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let hit = super.hitTest(point, with: event)
        if let hit, let onTouchHitTest, event?.type == .touches {
            onTouchHitTest(hit)
        }
        return hit
    }

    override func sendEvent(_ event: UIEvent) {
        super.sendEvent(event)
        guard event.type == .touches, let touches = event.allTouches else { return }
        for touch in touches where touch.phase == .ended {
            onTouchEnded?(touch)
        }
    }
}

func testWindowScene() -> UIWindowScene {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    precondition(scenes.count == 1, "NativeEditorTestHost must provide exactly one connected window scene")
    return scenes[0]
}

func makeTestWindow(frame: CGRect) -> UIWindow {
    let window = UIWindow(windowScene: testWindowScene())
    window.frame = frame
    return window
}

struct MountedEditorTestFixture {
    static let releaseTimeout: TimeInterval = 3

    let editorId: UInt64
    let adapter: EditorV2Adapter
    let view: RichTextEditorView
    let window: UIWindow?

    func close() {
        _ = view.activeTextInput.resignFirstResponder()
        view.editorId = 0
        view.removeFromSuperview()
        window?.isHidden = true
        destroyV2Editor(id: editorId)
    }
}

func makeMountedEditorFixture(
    config: String, document: String, size: CGSize, windowed: Bool = false
) throws -> MountedEditorTestFixture {
    let editorId = makeV2Editor(configJson: config)
    var finished = false
    defer { if !finished { destroyV2Editor(id: editorId) } }
    let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
    let frame = CGRect(origin: .zero, size: size)
    let window = windowed ? makeTestWindow(frame: frame) : nil
    let view = RichTextEditorView(frame: frame)
    window?.addSubview(view)
    window?.makeKeyAndVisible()
    view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
    XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
    finished = true
    return MountedEditorTestFixture(editorId: editorId, adapter: adapter, view: view, window: window)
}

extension XCTestCase {
    @MainActor
    func waitForClosedFixtureCallbacks(label: String) async -> XCTWaiter.Result {
        let callbacksDrained = expectation(description: label)
        DispatchQueue.main.async { callbacksDrained.fulfill() }
        return await XCTWaiter.fulfillment(of: [callbacksDrained], timeout: MountedEditorTestFixture.releaseTimeout)
    }
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
