import CoreText
import Foundation
import UIKit
import XCTest

extension PreparedProseLayoutTests {
    func testMixedViewerListsRetainBaseArithmeticAndResolveContainerAndMarkerPrefixes() throws {
        let outer = ViewerListContext(ordered: false, index: 1, kind: nil, checked: false, isLast: true)
        let inner = ViewerListContext(ordered: true, index: 2, kind: nil, checked: false, isLast: true)
        let block = ViewerBlock(nodeType: "paragraph", depth: 2, inBlockquote: true, listContext: inner,
            listItemBoundary: ViewerListItemBoundary(identity: 2, nestingDepth: 1, isFirstRenderableLeaf: true, isFinalRenderableLeaf: true),
            listItemAncestors: [ViewerListItemAncestor(identity: 1, context: outer), ViewerListItemAncestor(identity: 2, context: inner)],
            inlines: [.text(text: "nested", marks: [])],
            styleAncestors: ["blockquote", "bulletList", "listItem", "orderedList", "listItem"].enumerated().map { ViewerStyleAncestor(identity: $0.offset, nodeType: $0.element) })
        let document = ViewerDocument(semanticKey: String(repeating: "c", count: 64), blocks: [block], isEmpty: false, retainedBytes: 0, trailingEmptyTextBlockCount: 0)
        let registry = PreparedProseLayoutRegistry(compile: { _ in document })
        func measure(_ rules: String = "[]") -> PreparedProseBlock {
            let theme = """
            {"version":1,"styles":{"bulletList":{"indent":20,"baseIndentMultiplier":2},"orderedList":{"indent":40,"baseIndentMultiplier":3},"listMarker":{"gap":6}},"rules":\(rules)}
            """
            return registry.measure(request: ProseViewerRequest(source: .json("{}"), configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)), widthPoints: 600, scale: 2).blocks[0]
        }
        func fragment(_ block: PreparedProseBlock, _ kind: PreparedProseFragmentKind) throws -> PreparedProseFragment {
            try XCTUnwrap(block.fragments.first { $0.kind == kind })
        }
        let baseline = measure()
        XCTAssertEqual(try fragment(baseline, .text).bounds.minX - 19 - fragment(baseline, .marker).bounds.width - 6, 160, accuracy: 0.01)
        let indented = measure("""
        [{"path":["blockquote","bulletList"],"style":{"indent":33}}]
        """)
        XCTAssertEqual(try fragment(indented, .text).bounds.minX - fragment(baseline, .text).bounds.minX, 26, accuracy: 0.01)
        let styled = measure("""
        [{"path":["listItem","listMarker"],"style":{"gap":12,"color":"#00ff00ff","ordered":{"schemes":["upperAlpha"],"suffix":")"}}},{"path":["paragraph","listMarker"],"style":{"gap":99}}]
        """)
        let marker = try fragment(styled, .marker)
        XCTAssertEqual(marker.label, "B)")
        XCTAssertEqual(marker.color, UIColor.green.cgColor)
        XCTAssertEqual(try fragment(styled, .text).bounds.minX - fragment(baseline, .text).bounds.minX - marker.bounds.width + fragment(baseline, .marker).bounds.width, 6, accuracy: 0.01)
    }

    func testNestedViewerRulesUpdateGeometryPaintAndRestoreCachedBaseline() throws {
        let ancestors = [ViewerStyleAncestor(identity: 1, nodeType: "blockquote"), ViewerStyleAncestor(identity: 2, nodeType: "bulletList"), ViewerStyleAncestor(identity: 3, nodeType: "listItem")]
        let document = ViewerDocument(semanticKey: String(repeating: "a", count: 64), blocks: (0..<3).map { index in
            ViewerBlock(nodeType: "paragraph", depth: 0, inBlockquote: index < 2, listContext: nil, listItemBoundary: nil,
                inlines: [.text(text: "nested", marks: [FfiViewerMark(markType: "bold", attrsJson: "{}")])], styleAncestors: index < 2 ? ancestors : [])
        }, isEmpty: false, retainedBytes: 0, trailingEmptyTextBlockCount: 0)
        let styles = """
        {"text":{"lineHeight":20},"paragraph":{"marginTop":3,"marginBottom":8},"blockquote":{"paddingTop":2,"paddingBottom":4}}
        """
        let rules = """
        [
        {"path":["content"],"style":{"paddingLeft":5,"paddingTop":3}},
        {"path":["listItem","paragraph"],"style":{"marginTop":4,"marginBottom":0,"paddingTop":2,"paddingBottom":3,"fontSize":22,"lineHeight":40}},
        {"path":["blockquote","bulletList"],"style":{"paddingLeft":13,"paddingTop":5,"paddingBottom":6,"marginTop":11,"marginBottom":13}},
        {"path":["blockquote","bulletList","listItem","paragraph"],"style":{"paddingLeft":7}},
        {"path":["listItem","paragraph","bold"],"style":{"color":"#ff0000ff"}}
        ]
        """
        let registry = PreparedProseLayoutRegistry(compile: { _ in document })
        func measure(_ rules: String?) -> PreparedProseLayout {
            let ruleJSON = rules.map { ",\"rules\":\($0)" } ?? ""
            let theme = "{\"version\":1,\"styles\":\(styles)\(ruleJSON)}"
            return registry.measure(request: ProseViewerRequest(source: .json("{}"), configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)), widthPoints: 300, scale: 2)
        }
        func text(_ layout: PreparedProseLayout, _ index: Int) throws -> PreparedProseFragment {
            try XCTUnwrap(layout.blocks[index].fragments.first { $0.kind == .text })
        }
        let baseline = measure(nil)
        let styled = measure(rules)
        let restored = measure(nil)
        XCTAssertTrue(baseline === restored)
        XCTAssertEqual(registry.layoutPreparationCount, 2)
        XCTAssertEqual(styled.decorations.first?.styleBox?.padding.left, 5)
        XCTAssertNotEqual(baseline.key.themeDigest, styled.key.themeDigest)
        XCTAssertEqual(baseline.size, restored.size)
        XCTAssertEqual(try text(styled, 0).bounds.minX - text(baseline, 0).bounds.minX, 25, accuracy: 0.01)
        XCTAssertEqual(try text(styled, 0).bounds.minY - text(baseline, 0).bounds.minY, 22, accuracy: 0.01)
        XCTAssertEqual(try text(styled, 0).bounds.height, 40)
        XCTAssertEqual(try text(styled, 1).bounds.minY - text(styled, 0).bounds.maxY, 9, accuracy: 0.01)
        let paragraph = try XCTUnwrap(styled.blocks[0].fragments.first { $0.kind == .background })
        XCTAssertEqual(paragraph.styleBox?.margin.bottom, 0)
        XCTAssertEqual(paragraph.styleBox?.padding.left, 7)
        let list = try XCTUnwrap(styled.decorations.first { $0.styleBox?.padding.left == 13 })
        XCTAssertEqual(list.styleBox?.margin.top, 11)
        XCTAssertEqual(list.styleBox?.margin.bottom, 13)
        XCTAssertEqual(list.bounds.maxY, try text(styled, 1).bounds.maxY + 3 + 4 + 6, accuracy: 0.01)
        XCTAssertEqual(styled.blocks[2].fragments.first { $0.kind == .background }?.styleBox?.margin.bottom, 8)
        let run = try XCTUnwrap((CTLineGetGlyphRuns(try XCTUnwrap(text(styled, 0).line)) as? [CTRun])?.first)
        let attributes = CTRunGetAttributes(run) as NSDictionary
        XCTAssertEqual(attributes[kCTForegroundColorAttributeName] as! CGColor, UIColor.red.cgColor)
    }

    func testViewerSpecialElementsUseOwningAncestry() throws {
        let theme = """
        {"version":1,"styles":{"taskCheckbox":{"size":20,"checked":{"size":26}},"mention":{"paddingLeft":3}},"rules":[
        {"path":["taskItem","taskCheckbox"],"style":{"size":30,"gap":9,"checked":{"size":34}}},
        {"path":["paragraph","taskCheckbox"],"style":{"checked":{"size":99}}},
        {"path":["paragraph","bold"],"style":{"color":"#ff0000ff"}},
        {"path":["paragraph","mention"],"style":{"paddingLeft":17,"color":"#00ff00ff"}},
        {"path":["blockquote","horizontalRule"],"style":{"marginTop":19,"height":7,"backgroundColor":"#ff0000ff"}},
        {"path":["blockquote","image"],"style":{"paddingLeft":11,"resizeMode":"cover"}}
        ]}
        """
        let quote = ViewerStyleAncestor(identity: 1, nodeType: "blockquote")
        let document = ViewerDocument(semanticKey: String(repeating: "b", count: 64), blocks: [
            ViewerBlock(nodeType: "paragraph", depth: 1, inBlockquote: true,
                listContext: ViewerListContext(ordered: false, index: 1, kind: "task", checked: true, isLast: true),
                listItemBoundary: ViewerListItemBoundary(identity: 3, nestingDepth: 0, isFirstRenderableLeaf: true, isFinalRenderableLeaf: true),
                inlines: [.text(text: "marked", marks: [FfiViewerMark(markType: "bold", attrsJson: "{}")]), .atom(nodeType: "mention", docPos: 1, attrsJSON: "{}", label: "Ada")],
                styleAncestors: [quote, ViewerStyleAncestor(identity: 2, nodeType: "taskList"), ViewerStyleAncestor(identity: 3, nodeType: "taskItem")]),
            ViewerBlock(nodeType: "horizontalRule", depth: 1, inBlockquote: true, listContext: nil, listItemBoundary: nil, inlines: [], styleAncestors: [quote]),
            ViewerBlock(nodeType: "image", depth: 1, inBlockquote: true, listContext: nil, listItemBoundary: nil,
                inlines: [.atom(nodeType: "image", docPos: 3, attrsJSON: "{\"src\":\"test.png\",\"width\":40,\"height\":20}", label: "")], styleAncestors: [quote])
        ], isEmpty: false, retainedBytes: 0, trailingEmptyTextBlockCount: 0)
        let registry = PreparedProseLayoutRegistry(compile: { _ in document })
        let result = registry.measure(request: ProseViewerRequest(source: .json("{}"), configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)), widthPoints: 300, scale: 2)
        let marker = try XCTUnwrap(result.blocks[0].fragments.first { $0.kind == .marker })
        XCTAssertEqual(marker.bounds.width, 34)
        let atom = try XCTUnwrap(result.blocks[0].fragments.first { $0.kind == .atom })
        XCTAssertEqual(atom.styleBox?.padding.left, 17)
        let atomRun = try XCTUnwrap((CTLineGetGlyphRuns(try XCTUnwrap(atom.line)) as? [CTRun])?.first)
        XCTAssertEqual((CTRunGetAttributes(atomRun) as NSDictionary)[kCTForegroundColorAttributeName] as! CGColor, UIColor.green.cgColor)
        let rule = try XCTUnwrap(result.blocks[1].fragments.first { $0.kind == .background })
        XCTAssertEqual(rule.styleBox?.margin.top, 19)
        XCTAssertEqual(rule.bounds.height, 7)
        let image = try XCTUnwrap(result.blocks[2].fragments.first { $0.kind == .image })
        XCTAssertEqual(image.styleBox?.padding.left, 11)
        XCTAssertEqual(image.styleBox?.values["resizeMode"] as? String, "cover")
    }

    func testRegisteredBlockAtomReservesMeasuredWidthAndZeroHeight() throws {
        let block = ViewerBlock(
            nodeType: "card",
            depth: 0,
            inBlockquote: false,
            listContext: nil,
            listItemBoundary: nil,
            inlines: [.atom(nodeType: "card", docPos: 3, attrsJSON: "{}", label: "Card")],
            isBlockAtom: true
        )
        let document = ViewerDocument(
            semanticKey: String(repeating: "a", count: 64),
            blocks: [block],
            isEmpty: false,
            retainedBytes: 128,
            trailingEmptyTextBlockCount: 0
        )
        let registry = PreparedProseLayoutRegistry(compile: { _ in document })
        func measure(_ measuredWidth: Int) -> PreparedProseLayout {
            let theme = """
            {"viewerAtoms":{"generation":"g","revision":"r","nodeTypes":["card"],"estimatedHeights":{"card":70},"measurements":{"3":{"width":\(measuredWidth),"height":0}}}}
            """
            return registry.measure(
                request: ProseViewerRequest(
                    source: .json("{}"),
                    configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)
                ),
                widthPoints: 160,
                scale: 2
            )
        }
        XCTAssertEqual(measure(160).size.height, 0)
        XCTAssertEqual(measure(120).size.height, 70)
        XCTAssertTrue(measure(160).accessibilityNodes.isEmpty)
        let layout = measure(160)
        XCTAssertEqual(layout.blocks.first?.atomSlot?.bounds.width, 160)
        XCTAssertEqual(layout.blocks.first?.atomSlot?.docPos, 3)
        XCTAssertTrue(layout.blocks.first?.fragments.isEmpty == true)
        let drawing = PreparedProseDrawingView(frame: .zero)
        drawing.install(layout: layout)
        let data = drawing.atomLayoutsJSON(origin: CGPoint(x: 5, y: 9)).data(using: .utf8)!
        let atoms = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [[String: Any]])
        XCTAssertEqual(atoms.first?["x"] as? Double, 5)
        XCTAssertEqual(atoms.first?["y"] as? Double, 9)
    }

    func testShortListedViewerAtomsIncludeMarkerBoundsAndFollowingSpacing() throws {
        let atom = ViewerBlock(
            nodeType: "card",
            depth: 0,
            inBlockquote: false,
            listContext: ViewerListContext(ordered: false, index: 0, kind: nil, checked: false, isLast: true),
            listItemBoundary: nil,
            inlines: [.atom(nodeType: "card", docPos: 3, attrsJSON: "{}", label: "Card")],
            isBlockAtom: true
        )
        let paragraph = ViewerBlock(
            nodeType: "paragraph",
            depth: 0,
            inBlockquote: false,
            listContext: nil,
            listItemBoundary: nil,
            inlines: [.text(text: "After", marks: [])]
        )
        for height in [0, 1] {
            for followingParagraph in [false, true] {
                let document = ViewerDocument(
                    semanticKey: String(repeating: "a", count: 64),
                    blocks: followingParagraph ? [atom, paragraph] : [atom],
                    isEmpty: false,
                    retainedBytes: 128,
                    trailingEmptyTextBlockCount: 0
                )
                let registry = PreparedProseLayoutRegistry(compile: { _ in document })
                let theme = """
                {"viewerAtoms":{"nodeTypes":["card"],"estimatedHeights":{"card":\(height)}}}
                """
                let layout = registry.measure(
                    request: ProseViewerRequest(
                        source: .json("{}"),
                        configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)
                    ),
                    widthPoints: 200,
                    scale: 2
                )
                let block = try XCTUnwrap(layout.blocks.first)
                let marker = try XCTUnwrap(block.fragments.first { $0.kind == .marker })
                XCTAssertEqual(block.atomSlot?.bounds.height, CGFloat(height))
                XCTAssertGreaterThan(marker.bounds.height, CGFloat(height))
                XCTAssertTrue(block.bounds.contains(marker.bounds))
                XCTAssertGreaterThanOrEqual(layout.size.height, marker.bounds.maxY)
                if followingParagraph {
                    XCTAssertGreaterThanOrEqual(layout.blocks[1].bounds.minY, marker.bounds.maxY)
                }
            }
        }
    }

    func testViewerAtomDefaultsAndExactMeasurementWidth() {
        let atoms = PreparedViewerAtoms.resolve("""
        {"viewerAtoms":{"nodeTypes":["card"],"measurements":{"3":{"width":160.1,"height":90}}}}
        """)!
        XCTAssertEqual(atoms.height(nodeType: "card", docPos: 3, width: 160), 32)
        XCTAssertEqual(atoms.height(nodeType: "card", docPos: 3, width: 160.1), 90)
    }

    func testViewerAtomDecorationsFallbackAndDownstreamGeometry() {
        func block(
            _ nodeType: String,
            atom: Bool,
            quote: Bool = false,
            list: ViewerListContext? = nil
        ) -> ViewerBlock {
            ViewerBlock(
                nodeType: nodeType,
                depth: 0,
                inBlockquote: quote,
                listContext: list,
                listItemBoundary: nil,
                inlines: [.atom(nodeType: "card", docPos: 3, attrsJSON: "{}", label: "Card")],
                isBlockAtom: atom
            )
        }
        func measure(_ first: ViewerBlock, registered: Bool = true) -> PreparedProseLayout {
            let paragraph = ViewerBlock(
                nodeType: "paragraph",
                depth: 0,
                inBlockquote: false,
                listContext: nil,
                listItemBoundary: nil,
                inlines: [.text(text: "After", marks: [])]
            )
            let document = ViewerDocument(
                semanticKey: String(repeating: "a", count: 64),
                blocks: [first, paragraph],
                isEmpty: false,
                retainedBytes: 128,
                trailingEmptyTextBlockCount: 0
            )
            let registry = PreparedProseLayoutRegistry(compile: { _ in document })
            let theme = """
            {"viewerAtoms":{"nodeTypes":["\(registered ? "card" : "other")"],"estimatedHeights":{"card":80}}}
            """
            return registry.measure(
                request: ProseViewerRequest(
                    source: .json("{}"),
                    configuration: ProseViewerConfiguration(configJSON: "{}", themeJSON: theme)
                ),
                widthPoints: 200,
                scale: 2
            )
        }
        let plain = measure(block("card", atom: true))
        XCTAssertEqual(plain.blocks[0].atomSlot?.bounds.height, 80)
        XCTAssertGreaterThanOrEqual(plain.blocks[1].bounds.minY, 80)
        let decorated = measure(block(
            "card",
            atom: true,
            quote: true,
            list: ViewerListContext(ordered: false, index: 0, kind: nil, checked: false, isLast: true)
        ))
        XCTAssertTrue(decorated.blocks[0].fragments.contains { $0.kind == .marker })
        XCTAssertTrue(decorated.blocks[0].fragments.contains { $0.kind == .border })
        XCTAssertLessThan(decorated.blocks[0].atomSlot!.bounds.width, 200)
        XCTAssertGreaterThan(decorated.blocks[0].atomSlot!.bounds.minX, 0)
        let inline = measure(block("paragraph", atom: false))
        XCTAssertNil(inline.blocks[0].atomSlot)
        XCTAssertTrue(inline.blocks[0].fragments.contains { $0.kind == .atom })
        let fallback = measure(block("card", atom: true), registered: false)
        XCTAssertNil(fallback.blocks[0].atomSlot)
        XCTAssertTrue(fallback.blocks[0].fragments.contains { $0.kind == .atom })
    }

    func testCollapseTrailingEmptyParagraphs() {
        let blocks = ["first", "", "second", "", ""].map { text in
            ViewerBlock(
                nodeType: "paragraph",
                depth: 0,
                inBlockquote: false,
                listContext: nil,
                listItemBoundary: nil,
                inlines: [.text(text: text.isEmpty ? "\u{200B}" : text, marks: [])]
            )
        }
        let document = ViewerDocument(
            semanticKey: String(repeating: "a", count: 64),
            blocks: blocks,
            isEmpty: false,
            retainedBytes: 128,
            trailingEmptyTextBlockCount: 2
        )
        let registry = PreparedProseLayoutRegistry(compile: { _ in document })
        func request(collapse: Bool) -> ProseViewerRequest {
            ProseViewerRequest(
                source: .json("{}"),
                configuration: ProseViewerConfiguration(
                    configJSON: "{}",
                    collapsesWhenEmpty: collapse
                )
            )
        }

        let collapsed = registry.measure(
            request: request(collapse: true),
            widthPoints: 160,
            scale: 2
        )
        let expanded = registry.measure(
            request: request(collapse: false),
            widthPoints: 160,
            scale: 2
        )

        XCTAssertEqual(collapsed.blocks.count, 3)
        XCTAssertEqual(expanded.blocks.count, 5)
    }

    func testCollapseTrailingHiddenInlineImagePreservesPrecedingParagraph() {
        let source = """
        {"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"keep"}]},
        {"type":"paragraph","content":[{"type":"image","attrs":{"src":"https://example.test/image.png"}}]}
        ]}
        """
        let configJSON = """
        {"schema":{"nodes":[
        {"name":"doc","content":"block+","role":"doc"},
        {"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},
        {"name":"image","content":"","group":"inline","role":"inline","isVoid":true,"attrs":{"src":{}}},
        {"name":"text","group":"inline","role":"text"}
        ],"marks":[]},"initialization":{"type":"localEmpty"}}
        """
        let request = ProseViewerRequest(
            source: .json(source),
            configuration: ProseViewerConfiguration(
                configJSON: configJSON,
                imagesEnabled: false,
                collapsesWhenEmpty: true
            )
        )

        let layout = PreparedProseLayoutRegistry().measure(
            request: request,
            widthPoints: 160,
            scale: 2
        )

        XCTAssertNil(layout.error)
        XCTAssertEqual(layout.blocks.count, 1)
    }

    func testCustomBlockContainerDoesNotBecomeAnEmptyLeaf() {
        let source = """
        {"type":"doc","content":[
        {"type":"callout","content":[
            {"type":"paragraph","content":[{"type":"text","text":"keep"}]}
        ]}
        ]}
        """
        let configJSON = """
        {"schema":{"nodes":[
        {"name":"doc","content":"block+","role":"doc"},
        {"name":"callout","content":"block+","group":"block","role":"block"},
        {"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},
        {"name":"text","group":"inline","role":"text"}
        ],"marks":[]},"initialization":{"type":"localEmpty"}}
        """
        let request = ProseViewerRequest(
            source: .json(source),
            configuration: ProseViewerConfiguration(
                configJSON: configJSON,
                imagesEnabled: false,
                collapsesWhenEmpty: false
            )
        )

        let layout = PreparedProseLayoutRegistry().measure(
            request: request,
            widthPoints: 160,
            scale: 2
        )

        XCTAssertNil(layout.error)
        XCTAssertEqual(layout.blocks.count, 1)
    }

    func testOrderedListMarkersUseDefaultSchemesBySemanticNestingDepth() throws {
        let orderedContext = ViewerListContext(
            ordered: true,
            index: 1,
            kind: nil,
            checked: false,
            isLast: true
        )
        let bulletAncestor = ViewerListItemAncestor(
            identity: 100,
            context: ViewerListContext(
                ordered: false,
                index: 1,
                kind: nil,
                checked: false,
                isLast: true
            )
        )
        let nestedOrderedAncestor = ViewerListItemAncestor(
            identity: 101,
            context: orderedContext
        )
        let nestedBulletAncestor = ViewerListItemAncestor(
            identity: 102,
            context: bulletAncestor.context
        )
        let ancestorChains = [
            [ViewerListItemAncestor(identity: 0, context: orderedContext)],
            [
                bulletAncestor,
                ViewerListItemAncestor(identity: 1, context: orderedContext)
            ],
            [
                nestedOrderedAncestor,
                nestedBulletAncestor,
                ViewerListItemAncestor(identity: 2, context: orderedContext)
            ],
            [
                bulletAncestor,
                nestedOrderedAncestor,
                nestedBulletAncestor,
                ViewerListItemAncestor(identity: 3, context: orderedContext)
            ]
        ]
        let mismatchedBoundaryDepths: [UInt16] = [2, 0, 0, 1]
        let blocks = ancestorChains.enumerated().map { index, ancestors in
            ViewerBlock(
                nodeType: "paragraph",
                depth: UInt16(40 + index),
                inBlockquote: index == 1,
                listContext: orderedContext,
                listItemBoundary: ViewerListItemBoundary(
                    identity: ancestors.last!.identity,
                    nestingDepth: mismatchedBoundaryDepths[index],
                    isFirstRenderableLeaf: true,
                    isFinalRenderableLeaf: true
                ),
                listItemAncestors: ancestors,
                inlines: [.text(text: "item", marks: [])]
            )
        }
        let document = ViewerDocument(
            semanticKey: String(repeating: "a", count: 64),
            blocks: blocks,
            isEmpty: false,
            retainedBytes: 128,
            preparedTheme: PreparedProseTheme.resolve(themeJSON: nil)
        )
        let key = ProseLayoutKey(
            semanticKey: document.semanticKey,
            widthPixels: 640,
            themeDigest: "ordered-marker-theme",
            nativeFontRevision: 0,
            fontEnvironmentRevision: 0,
            displayScale: 2,
            attachmentRevision: 0,
            generationIdentity: "ordered-marker-theme",
            semanticGenerationIdentity: "ordered-marker-theme"
        )

        let layout = try CoreTextProseLayoutEngine().prepare(
            document: document,
            key: key,
            widthPoints: 320,
            displayScale: 2
        )
        let markerLabels = layout.blocks
            .flatMap(\.fragments)
            .filter { $0.kind == .marker }
            .compactMap(\.label)

        XCTAssertEqual(markerLabels, ["1.", "a.", "i.", "1."])
    }

    func testOrderedListFallbackUsesSemanticAncestorDepth() throws {
        let orderedContext = ViewerListContext(
            ordered: true,
            index: 1,
            kind: nil,
            checked: false,
            isLast: true
        )
        let bulletContext = ViewerListContext(
            ordered: false,
            index: 1,
            kind: nil,
            checked: false,
            isLast: true
        )
        let block = ViewerBlock(
            nodeType: "paragraph",
            depth: 64,
            inBlockquote: true,
            listContext: orderedContext,
            listItemBoundary: nil,
            listItemAncestors: [
                ViewerListItemAncestor(identity: 100, context: bulletContext),
                ViewerListItemAncestor(identity: 101, context: orderedContext),
                ViewerListItemAncestor(identity: 102, context: orderedContext)
            ],
            inlines: [.text(text: "item", marks: [])]
        )
        let themeJSON = """
        {"list":{"orderedMarker":{"schemes":["decimal","lowerAlpha","lowerRoman"],"suffix":")"}}}
        """
        let document = ViewerDocument(
            semanticKey: String(repeating: "a", count: 64),
            blocks: [block],
            isEmpty: false,
            retainedBytes: 128,
            preparedTheme: PreparedProseTheme.resolve(themeJSON: themeJSON)
        )
        let key = ProseLayoutKey(
            semanticKey: document.semanticKey,
            widthPixels: 640,
            themeDigest: "ordered-marker-theme",
            nativeFontRevision: 0,
            fontEnvironmentRevision: 0,
            displayScale: 2,
            attachmentRevision: 0,
            generationIdentity: "ordered-marker-theme",
            semanticGenerationIdentity: "ordered-marker-theme"
        )

        let layout = try CoreTextProseLayoutEngine().prepare(
            document: document,
            key: key,
            widthPoints: 320,
            displayScale: 2
        )
        let markerLabels = layout.blocks
            .flatMap(\.fragments)
            .filter { $0.kind == .marker }
            .compactMap(\.label)

        XCTAssertEqual(markerLabels, ["i)"])
    }

    func testOrderedMarkerEditorAndViewerRenderingConformForSharedTuples() throws {
        struct MarkerFixture {
            let index: Int
            let semanticDepth: Int
            let expected: String
        }
        let fixtures: [MarkerFixture] = [
            MarkerFixture(index: 27, semanticDepth: 0, expected: "AA)"),
            MarkerFixture(index: 3_999, semanticDepth: 1, expected: "MMMCMXCIX)"),
            MarkerFixture(index: 42, semanticDepth: 2, expected: "42)")
        ]
        let themeDictionary: [String: Any] = [
            "list": [
                "orderedMarker": [
                    "schemes": ["upperAlpha", "upperRoman", "decimal"],
                    "suffix": ")"
                ]
            ]
        ]
        let themeJSONData = try JSONSerialization.data(withJSONObject: themeDictionary)
        let themeJSON = try XCTUnwrap(String(data: themeJSONData, encoding: .utf8))
        let editorTheme = EditorTheme(dictionary: themeDictionary)
        let viewerTheme = PreparedProseTheme.resolve(themeJSON: themeJSON)

        for fixture in fixtures {
            var elements: [[String: Any]] = []
            for depth in 0...fixture.semanticDepth {
                let deepest = depth == fixture.semanticDepth
                elements.append([
                    "type": "blockStart",
                    "nodeType": "listItem",
                    "depth": depth,
                    "listContext": [
                        "ordered": deepest,
                        "index": deepest ? fixture.index : 1,
                        "isFirst": true,
                        "isLast": true
                    ]
                ])
            }
            elements.append([
                "type": "blockStart",
                "nodeType": "paragraph",
                "depth": fixture.semanticDepth + 1
            ])
            elements.append(["type": "textRun", "text": "item", "marks": []])
            elements.append(["type": "blockEnd"])
            for _ in 0...fixture.semanticDepth {
                elements.append(["type": "blockEnd"])
            }
            let renderData = try JSONSerialization.data(withJSONObject: elements)
            let renderJSON = try XCTUnwrap(String(data: renderData, encoding: .utf8))
            let editor = RenderBridge.renderElements(
                fromJSON: renderJSON,
                baseFont: .systemFont(ofSize: 16),
                textColor: .label,
                theme: editorTheme
            )
            let editorLabel = editor.attribute(
                RenderBridgeAttributes.orderedListMarkerLabel,
                at: 0,
                effectiveRange: nil
            ) as? String

            let orderedContext = ViewerListContext(
                ordered: true,
                index: fixture.index,
                kind: nil,
                checked: false,
                isLast: true
            )
            let ancestors = (0...fixture.semanticDepth).map { depth in
                ViewerListItemAncestor(
                    identity: depth,
                    context: depth == fixture.semanticDepth
                        ? orderedContext
                        : ViewerListContext(
                            ordered: false,
                            index: 1,
                            kind: nil,
                            checked: false,
                            isLast: true
                        )
                )
            }
            let block = ViewerBlock(
                nodeType: "paragraph",
                depth: UInt16(80 + fixture.semanticDepth),
                inBlockquote: false,
                listContext: orderedContext,
                listItemBoundary: ViewerListItemBoundary(
                    identity: ancestors.last!.identity,
                    nestingDepth: UInt16(40 - fixture.semanticDepth),
                    isFirstRenderableLeaf: true,
                    isFinalRenderableLeaf: true
                ),
                listItemAncestors: ancestors,
                inlines: [.text(text: "item", marks: [])]
            )
            let semanticKey = "conformance-\(fixture.semanticDepth)"
            let viewer = try CoreTextProseLayoutEngine().prepare(
                document: ViewerDocument(
                    semanticKey: semanticKey,
                    blocks: [block],
                    isEmpty: false,
                    retainedBytes: 64,
                    preparedTheme: viewerTheme
                ),
                key: ProseLayoutKey(
                    semanticKey: semanticKey,
                    widthPixels: 640,
                    themeDigest: "ordered-marker-conformance",
                    nativeFontRevision: 0,
                    fontEnvironmentRevision: 0,
                    displayScale: 2,
                    attachmentRevision: 0,
                    generationIdentity: semanticKey,
                    semanticGenerationIdentity: semanticKey
                ),
                widthPoints: 320,
                displayScale: 2
            )
            let viewerLabel = viewer.blocks
                .flatMap(\.fragments)
                .first { $0.kind == .marker }?
                .label

            XCTAssertEqual(editorLabel, fixture.expected)
            XCTAssertEqual(viewerLabel, fixture.expected)
            XCTAssertEqual(editorLabel, viewerLabel)
        }
    }

}
