import ExpoModulesCore
import XCTest

@MainActor
final class TestTextDragSession: NSObject, UIDragSession {
    let items: [UIDragItem]
    var localContext: Any?
    var windowLocation: CGPoint?
    var allowsMoveOperation: Bool { true }
    var isRestrictedToDraggingApplication: Bool { true }

    init(items: [UIDragItem], windowLocation: CGPoint? = nil) {
        self.items = items
        self.windowLocation = windowLocation
    }

    func location(in view: UIView) -> CGPoint {
        windowLocation.map { view.convert($0, from: nil) } ?? .zero
    }

    func hasItemsConforming(toTypeIdentifiers typeIdentifiers: [String]) -> Bool {
        items.contains { item in
            typeIdentifiers.contains { item.itemProvider.hasItemConformingToTypeIdentifier($0) }
        }
    }

    func canLoadObjects(ofClass aClass: NSItemProviderReading.Type) -> Bool {
        items.contains { $0.itemProvider.canLoadObject(ofClass: aClass) }
    }
}

@MainActor
final class TestTextDropSession: NSObject, UIDropSession {
    let items: [UIDragItem]
    let localDragSession: UIDragSession?
    var progressIndicatorStyle: UIDropSessionProgressIndicatorStyle = .default
    let progress = Progress(totalUnitCount: 1)
    var allowsMoveOperation: Bool { true }
    var isRestrictedToDraggingApplication: Bool { true }

    var windowLocation: CGPoint?

    init(dragSession: UIDragSession, windowLocation: CGPoint? = nil) {
        localDragSession = dragSession
        items = dragSession.items
        self.windowLocation = windowLocation
    }

    init(externalItems: [UIDragItem], windowLocation: CGPoint) {
        localDragSession = nil
        items = externalItems
        self.windowLocation = windowLocation
    }

    func location(in view: UIView) -> CGPoint {
        windowLocation.map { view.convert($0, from: nil) } ?? .zero
    }

    func hasItemsConforming(toTypeIdentifiers typeIdentifiers: [String]) -> Bool {
        items.contains { item in
            typeIdentifiers.contains { item.itemProvider.hasItemConformingToTypeIdentifier($0) }
        }
    }

    func canLoadObjects(ofClass aClass: NSItemProviderReading.Type) -> Bool {
        items.contains { $0.itemProvider.canLoadObject(ofClass: aClass) }
    }

    func loadObjects(
        ofClass aClass: NSItemProviderReading.Type,
        completion: @escaping ([NSItemProviderReading]) -> Void
    ) -> Progress {
        completion([])
        return progress
    }
}

@MainActor
final class TestTextDragRequest: NSObject, UITextDragRequest {
    let dragRange: UITextRange
    let suggestedItems: [UIDragItem]
    let existingItems: [UIDragItem] = []
    let isSelected: Bool
    let dragSession: UIDragSession

    init(
        dragRange: UITextRange,
        suggestedItems: [UIDragItem],
        isSelected: Bool,
        dragSession: UIDragSession
    ) {
        self.dragRange = dragRange
        self.suggestedItems = suggestedItems
        self.isSelected = isSelected
        self.dragSession = dragSession
    }
}

@MainActor
final class TestTextDropRequest: NSObject, UITextDropRequest {
    let dropPosition: UITextPosition
    let suggestedProposal: UITextDropProposal
    let isSameView: Bool
    let dropSession: UIDropSession

    init(
        dropPosition: UITextPosition,
        isSameView: Bool,
        dropSession: UIDropSession
    ) {
        self.dropPosition = dropPosition
        self.isSameView = isSameView
        self.dropSession = dropSession
        suggestedProposal = UITextDropProposal(operation: .copy)
    }
}

// MARK: - v2 view integration tests (formerly the staging-variant suite)
//
// The view is bound to a v2 session through the session pairing registry, so
// every interaction — typing, marked text, autocorrect, selection, toolbar,
// accessibility-style edits, render patches — flows through the typed v2
// transactions. This is the only engine path: no legacy runtime exists.
final class EditorV2StagingViewTests: XCTestCase {

    static let atomBoundaryBackspaceAllowance = 2

    var adapters: [EditorV2Adapter] = []
    var syntheticIds: [UInt64] = []

    override func tearDown() {
        for id in syntheticIds {
            EditorV2Registry.destroyPair(forLegacyId: id)
        }
        syntheticIds = []
        adapters = []
        super.tearDown()
    }

    private func hostStagingView(_ view: RichTextEditorView) -> UIWindow {
        let window = makeTestWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        let viewController = UIViewController()
        window.rootViewController = viewController
        window.makeKeyAndVisible()
        viewController.view.addSubview(view)
        view.layoutIfNeeded()
        return window
    }

    struct BoundView {
        let view: RichTextEditorView
        let adapter: EditorV2Adapter
        let window: UIWindow
    }

    func makeBoundView(
        configJson: String = #"{"initialization":{"type":"localEmpty"}}"#,
        html: String = "<p>Hello</p>",
        initialFrame: CGRect = CGRect(x: 0, y: 0, width: 320, height: 120),
        finalFrame: CGRect? = nil,
        file: StaticString = #filePath,
        line: UInt = #line
    ) -> BoundView {
        let syntheticId = makeV2Editor(configJson: configJson, file: file, line: line)
        guard let adapter = EditorV2Registry.adapter(forLegacyId: syntheticId) else {
            XCTFail("v2 adapter was not paired to its created handle", file: file, line: line)
            fatalError("unreachable")
        }
        adapters.append(adapter)
        syntheticIds.append(syntheticId)
        let view = RichTextEditorView(frame: initialFrame)
        let window = hostStagingView(view)
        view.editorId = syntheticId
        view.setContent(html: html)
        if let finalFrame {
            view.frame = finalFrame
            view.layoutIfNeeded()
        }
        return BoundView(view: view, adapter: adapter, window: window)
    }

    func makeTerminalAtomView(
        html: String = #"<div data-type="counter-card" data-count="7"></div>"#,
        initialFrame: CGRect = CGRect(x: 0, y: 0, width: 320, height: 120),
        finalFrame: CGRect? = nil,
        file: StaticString = #filePath,
        line: UInt = #line
    ) -> BoundView {
        let configJson = #"""
        {
        "initialization":{"type":"localEmpty"},
        "schema":{
            "nodes":[
            {"name":"doc","content":"block+","role":"doc"},
            {"name":"paragraph","content":"text*","group":"block","role":"textBlock","htmlTag":"p"},
            {"name":"text","content":"","role":"text"},
            {"name":"ordered_list","content":"list_item+","group":"block","role":"list","htmlTag":"ol","attrs":{"start":{"default":1}}},
            {"name":"list_item","content":"paragraph block*","role":"listItem"},
            {"name":"horizontal_rule","content":"","group":"block","role":"block","isVoid":true,"htmlTag":"hr"},
            {
                "name":"counterCard",
                "content":"",
                "group":"block",
                "role":"block",
                "isVoid":true,
                "attrs":{"count":{"default":0}},
                "html":{
                "tag":"div",
                "staticAttrs":{"data-type":"counter-card"},
                "attrMap":{"count":"data-count"}
                }
            }
            ],
            "marks":[]
        }
        }
        """#
        let bound = makeBoundView(
            configJson: configJson,
            html: html,
            initialFrame: initialFrame,
            finalFrame: finalFrame,
            file: file,
            line: line
        )
        XCTAssertTrue(
            bound.view.applyAtomRenderConfiguration(
                AtomRenderConfiguration(
                    registeredNodeTypes: ["counterCard"],
                    estimatedHeights: ["counterCard": 72],
                    measuredHeights: [:]
                )
            ),
            file: file,
            line: line
        )
        bound.view.layoutIfNeeded()
        return bound
    }

    func terminalAtomRect(
        in textView: EditorTextView,
        file: StaticString = #filePath,
        line: UInt = #line
    ) -> CGRect {
        guard textView.textStorage.length > 0 else {
            XCTFail("expected terminal atom text", file: file, line: line)
            return .zero
        }
        let range = NSRange(location: textView.textStorage.length - 1, length: 1)
        let glyphRange = textView.layoutManager.glyphRange(
            forCharacterRange: range,
            actualCharacterRange: nil
        )
        let rect = textView.layoutManager.boundingRect(
            forGlyphRange: glyphRange,
            in: textView.textContainer
        )
        return rect.offsetBy(
            dx: textView.textContainerInset.left,
            dy: textView.textContainerInset.top
        )
    }

    func flushMain() {
        let expectation = expectation(description: "flush main")
        DispatchQueue.main.async { expectation.fulfill() }
        wait(for: [expectation], timeout: 1.0)
    }

    func flushMain(until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(1.0)
        repeat {
            flushMain()
        } while !condition() && Date() < deadline
    }

    func setCollapsedCaret(in textView: UITextView, utf16Offset: Int) {
        textView.selectedRange = NSRange(location: utf16Offset, length: 0)
    }

    func v2DocumentJSON(_ adapter: EditorV2Adapter) throws -> NSDictionary {
        let json = try XCTUnwrap(adapter.documentJson())
        let data = try XCTUnwrap(json.data(using: .utf8))
        return try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? NSDictionary)
    }

    func v2DocumentText(_ adapter: EditorV2Adapter, file: StaticString = #filePath, line: UInt = #line) -> String {
        guard let doc = try? v2DocumentJSON(adapter) as? [String: Any] else {
            XCTFail("getDocumentJson did not return a valid document object", file: file, line: line)
            return ""
        }
        var pieces: [String] = []
        func walk(_ node: [String: Any]) {
            if let type = node["type"] as? String, type == "text", let text = node["text"] as? String {
                pieces.append(text)
            }
            for child in (node["content"] as? [[String: Any]]) ?? [] { walk(child) }
        }
        walk(doc)
        return pieces.joined()
    }

}
