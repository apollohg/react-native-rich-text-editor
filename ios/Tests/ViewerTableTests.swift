import CoreText
import XCTest

final class ViewerTableTests: XCTestCase {
    func testMountedViewportBoundsTableCandidatesWithoutRepreparingCells() throws {
        let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_header","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#
        var result = viewerCompile(request: FfiViewerCompileRequest(sourceKind: .json, source: source, configJson: Self.config, imagesEnabled: true, mentionPrefix: nil))
        let document = try ViewerDocument(compiled: try XCTUnwrap(result.value))
        result.value = nil
        let engine = CoreTextProseLayoutEngine()
        var preparations: [Int] = []
        engine.tableCellPreparationObserver = { preparations.append($0) }
        let layout = try prepare(document, engine: engine)
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertGreaterThan(surface.bounds.width, surface.hostViewportWidth)
        let childIdentities = surface.cells.map { ObjectIdentifier($0.content) }
        XCTAssertEqual(surface.cells.count, Set(childIdentities).count)
        let originalSourceOrder = surface.layout.sourceOrder
        XCTAssertEqual(Set(surface.cells.map(\.sourcePosition)), Set(preparations))
        let initialPreparations = preparations.count
        XCTAssertGreaterThan(initialPreparations, 0)

        let drawing = PreparedProseDrawingView(frame: CGRect(x: 0, y: 0, width: 120, height: 45))
        drawing.install(layout: layout)
        let window = UIWindow(frame: drawing.bounds)
        window.addSubview(drawing)
        window.isHidden = false
        defer { window.isHidden = true }
        var mounted: [Int] = []
        var chrome: [Int] = []
        var rich = 0
        drawing.onMountedTableCellsDrawnForTesting = { mounted.append($0) }
        drawing.onTableChromeDrawnForTesting = { chrome.append($0) }
        drawing.onTableRichFragmentDrawnForTesting = { rich += 1 }
        let format = UIGraphicsImageRendererFormat(); format.scale = 1
        func draw(_ rect: CGRect) {
            _ = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { context in
                context.cgContext.clip(to: rect)
                drawing.draw(rect)
            }
        }
        draw(CGRect(x: 0, y: 0, width: 2, height: 2))
        XCTAssertEqual(2, mounted.last)
        XCTAssertEqual([surface.layout.sourceOrder[0], surface.layout.sourceOrder[2]], chrome)
        XCTAssertEqual(initialPreparations, preparations.count)
        chrome.removeAll(); rich = 0
        draw(drawing.bounds)
        XCTAssertEqual([surface.layout.sourceOrder[0], surface.layout.sourceOrder[2]], chrome)
        XCTAssertGreaterThan(rich, 0)
        XCTAssertLessThan(rich, surface.cells.count)
        drawing.setTableLogicalOffset(800, sourceIdentity: surface.identity)
        chrome.removeAll()
        draw(CGRect(x: 0, y: 0, width: 2, height: 2))
        XCTAssertEqual([surface.layout.sourceOrder[1], surface.layout.sourceOrder[3]], chrome)
        XCTAssertEqual(initialPreparations, preparations.count)
        drawing.setTableLogicalOffset(0, sourceIdentity: surface.identity)
        chrome.removeAll(); rich = 0
        draw(drawing.bounds)
        XCTAssertGreaterThan(rich, 0)
        drawing.frame.origin.y = 10_000; chrome.removeAll(); rich = 0
        draw(drawing.bounds)
        XCTAssertEqual(0, mounted.last); XCTAssertTrue(chrome.isEmpty); XCTAssertEqual(0, rich)
        drawing.frame.origin.y = 0
        drawing.isHidden = true; chrome.removeAll(); rich = 0
        draw(drawing.bounds)
        XCTAssertEqual(0, mounted.last); XCTAssertTrue(chrome.isEmpty); XCTAssertEqual(0, rich)

        let detached = PreparedProseDrawingView(frame: drawing.bounds)
        detached.install(layout: layout)
        var detachedMounted: [Int] = []
        var detachedChrome: [Int] = []
        detached.onMountedTableCellsDrawnForTesting = { detachedMounted.append($0) }
        detached.onTableChromeDrawnForTesting = { detachedChrome.append($0) }
        _ = UIGraphicsImageRenderer(size: detached.bounds.size, format: format).image { _ in
            detached.draw(detached.bounds)
        }
        XCTAssertEqual(1, detachedMounted.count)
        XCTAssertEqual(surface.cells.count, detachedMounted[0])
        XCTAssertEqual(originalSourceOrder, detachedChrome)
        XCTAssertEqual(initialPreparations, preparations.count)
        XCTAssertEqual(originalSourceOrder, surface.layout.sourceOrder)
        XCTAssertEqual(childIdentities, surface.cells.map { ObjectIdentifier($0.content) })
    }
    func testDrawingViewPaintsNestedHeaderImageFromRootIdentity() throws {
        let source = try nestedHeaderImageSource()
        let layout = try prepare(source, themeJSON: ##"{"version":1,"styles":{"blockquote":{"backgroundColor":"#00ff00ff"}}}"##)
        let outer = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(outer.tableSurface)
        let image = try XCTUnwrap(layout.imageAttachments.first { $0.source == "https://example.test/nested.png" })
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let imagePixels = UIGraphicsImageRenderer(size: CGSize(width: 20, height: 20), format: format).image { context in
            UIColor.red.setFill(); context.fill(CGRect(x: 0, y: 0, width: 10, height: 10))
            UIColor.blue.setFill(); context.fill(CGRect(x: 10, y: 10, width: 10, height: 10))
        }
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        drawing.imagePixels = [image.id: imagePixels]
        let rendered = try XCTUnwrap(UIGraphicsImageRenderer(size: layout.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }.cgImage)
        func colorRGBA(_ color: UIColor) -> [UInt8] {
            var red: CGFloat = 0
            var green: CGFloat = 0
            var blue: CGFloat = 0
            var alpha: CGFloat = 0
            XCTAssertTrue(color.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
            return [red, green, blue, alpha].map { UInt8(($0 * 255).rounded()) }
        }
        let header = try XCTUnwrap(surface.cells.first { $0.isHeader })
        let headerPoint = CGPoint(x: outer.tableBounds!.minX + header.frame.minX + 2, y: outer.tableBounds!.minY + header.frame.minY + 2)
        let headerPixel = try rgba(rendered, headerPoint)
        let snapshot = ViewerTablePresentation.project(layout: layout, owner: ViewerTablePresentationOwner(), viewport: .unknown)
        let projectedImage = try XCTUnwrap(
            snapshot
                .images.first { $0.attachment.id == image.id }
        )
        let nestedHeader = try XCTUnwrap(snapshot.cells.first {
            $0.surface !== surface && $0.cell.isHeader
        })
        let sourceImage = try XCTUnwrap(imagePixels.cgImage)
        XCTAssertEqual(try rgba(sourceImage, CGPoint(x: 4, y: 4)), [255, 0, 0, 255])
        XCTAssertEqual(try rgba(sourceImage, CGPoint(x: 15, y: 15)), [0, 0, 255, 255])
        XCTAssertEqual(headerPixel, colorRGBA(surface.style.headerBackgroundColor))
        XCTAssertEqual(
            try rgba(rendered, CGPoint(x: nestedHeader.bounds.minX + 2, y: nestedHeader.bounds.minY + 2)),
            colorRGBA(nestedHeader.surface.style.headerBackgroundColor)
        )
        for point in [CGPoint(x: 4, y: 4), CGPoint(x: 15, y: 15)] {
            XCTAssertEqual(
                try rgba(rendered, CGPoint(x: projectedImage.bounds.minX + point.x, y: projectedImage.bounds.minY + point.y)),
                try rgba(sourceImage, point)
            )
        }
        let quote = try XCTUnwrap(snapshot.layouts.lazy.compactMap { presented -> CGRect? in
            guard let decoration = presented.layout.decorations.first(where: {
                $0.styleBox?.color("backgroundColor") == UIColor.green
            }) else { return nil }
            return decoration.bounds.offsetBy(dx: presented.origin.x, dy: presented.origin.y)
        }.first)
        let quotePoint = CGPoint(x: quote.maxX - 2, y: quote.minY + 2)
        XCTAssertEqual(try rgba(rendered, quotePoint), [0, 255, 0, 255])
        let rootProse = try XCTUnwrap(layout.blocks.first { $0 !== outer })
        let proseBounds = try XCTUnwrap(rootProse.fragments.first { $0.kind == .text }?.bounds)
        let proseHasInk = (Int(proseBounds.minY)..<Int(proseBounds.maxY)).contains { y in
            (Int(proseBounds.minX)..<Int(proseBounds.maxX)).contains { x in
                let pixel = try! rgba(rendered, CGPoint(x: x, y: y))
                return pixel[3] > 0 && pixel[0] < 200 && pixel[1] < 200 && pixel[2] < 200
            }
        }
        XCTAssertTrue(proseHasInk)
        XCTAssertEqual(layout.imageAttachments.filter { $0.id == image.id }.count, 1)
    }

    func testMountedTableOffsetRefreshesVisibleImageWithoutManualRefresh() throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let sourcePixels = UIGraphicsImageRenderer(size: CGSize(width: 20, height: 20), format: format).image { context in
            UIColor.red.setFill(); context.fill(CGRect(x: 0, y: 0, width: 10, height: 10))
            UIColor.blue.setFill(); context.fill(CGRect(x: 10, y: 10, width: 10, height: 10))
        }
        let source = try nestedHeaderImageSource(
            imageSource: "data:image/png;base64,\(try XCTUnwrap(sourcePixels.pngData()).base64EncodedString())",
            nestedOverflow: true
        )
        let layout = try prepare(source, themeJSON: ##"{"version":1,"styles":{"blockquote":{"backgroundColor":"#00ff00ff"}}}"##)
        let outer = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(outer.tableSurface)
        let image = try XCTUnwrap(layout.imageAttachments.first)
        XCTAssertEqual(layout.imageAttachments.filter { $0.id == image.id }.count, 1)
        XCTAssertEqual(image.declaredSize, CGSize(width: 20, height: 20))
        let initialSnapshot = ViewerTablePresentation.project(
                layout: layout,
                owner: ViewerTablePresentationOwner(),
                viewport: .unknown
            )
        let projectedImage = try XCTUnwrap(initialSnapshot.images.first { $0.attachment.id == image.id })
        let nestedSurface = try XCTUnwrap(initialSnapshot.cells.first { $0.surface !== surface }?.surface)
        let fixedHostLeft = try XCTUnwrap(initialSnapshot.cells.first { $0.surface === nestedSurface }?.clip.minX)
        XCTAssertGreaterThan(fixedHostLeft, 0)
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        drawing.configureImages(generation: "table-offset", imagesEnabled: true, policyJSON: nil)
        let window = UIWindow(frame: CGRect(origin: .zero, size: layout.size))
        window.addSubview(drawing)
        window.isHidden = false
        defer {
            drawing.cancelConfiguredImages()
            window.isHidden = true
        }

        drawing.updateConfiguredImagesForVisibleWindow()
        flushMain(until: { drawing.imagePixels[image.id] != nil })
        XCTAssertNotNil(drawing.imagePixels[image.id])

        let partialOffset = projectedImage.bounds.minX - fixedHostLeft + projectedImage.bounds.width / 2
        drawing.setTableLogicalOffset(partialOffset, sourceIdentity: nestedSurface.identity)
        let partialOwner = ViewerTablePresentationOwner()
        partialOwner.setLogicalOffset(partialOffset, for: nestedSurface)
        let partial = try XCTUnwrap(ViewerTablePresentation.project(layout: layout, owner: partialOwner, viewport: .unknown).images.first { $0.attachment.id == image.id })
        let visiblePartial = partial.bounds.intersection(partial.clip)
        XCTAssertGreaterThan(visiblePartial.width, 0)
        XCTAssertLessThan(visiblePartial.width, partial.bounds.width)
        let rendered = try XCTUnwrap(
            UIGraphicsImageRenderer(size: layout.size, format: format).image { _ in
                drawing.draw(drawing.bounds)
            }.cgImage
        )
        XCTAssertEqual(
            try rgba(
                rendered,
                CGPoint(
                    x: projectedImage.bounds.minX + projectedImage.bounds.width * 0.75 - partialOffset,
                    y: projectedImage.bounds.minY + projectedImage.bounds.height * 0.75
                )
            ),
            [0, 0, 255, 255]
        )
        let withoutImage = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        withoutImage.install(layout: layout)
        withoutImage.setTableLogicalOffset(partialOffset, sourceIdentity: nestedSurface.identity)
        let baseline = try XCTUnwrap(UIGraphicsImageRenderer(size: layout.size, format: format).image { _ in
            withoutImage.draw(withoutImage.bounds)
        }.cgImage)
        let outside = CGPoint(
            x: projectedImage.bounds.minX + projectedImage.bounds.width * 0.45 - partialOffset,
            y: projectedImage.bounds.minY + projectedImage.bounds.height * 0.2
        )
        XCTAssertGreaterThanOrEqual(outside.x, 0)
        XCTAssertLessThan(outside.x, partial.clip.minX)
        XCTAssertEqual(try rgba(baseline, outside), try rgba(rendered, outside))

        let fullyClippedOffset = projectedImage.bounds.maxX - fixedHostLeft + 1
        XCTAssertLessThanOrEqual(fullyClippedOffset, nestedSurface.bounds.width - nestedSurface.hostViewportWidth)
        drawing.setTableLogicalOffset(fullyClippedOffset, sourceIdentity: nestedSurface.identity)

        XCTAssertNil(drawing.imagePixels[image.id])

        drawing.setTableLogicalOffset(0, sourceIdentity: nestedSurface.identity)
        flushMain(until: { drawing.imagePixels[image.id] != nil })
        XCTAssertNotNil(drawing.imagePixels[image.id])
        XCTAssertEqual(try XCTUnwrap(layout.imageAttachments.first { $0.id == image.id }).bounds, image.bounds)
    }

    func testCompilerBackedTableAccessibilityUsesCurrentRootAndClipsOffsetElements() throws {
        let config = Self.config.replacingOccurrences(of: "\"marks\":[{\"name\":\"bold\"}]", with: "\"marks\":[{\"name\":\"bold\"},{\"name\":\"link\",\"attrs\":{\"href\":{\"default\":\"\"}}}]")
        func link(_ href: String) -> [String: Any] {
            ["type": "paragraph", "content": [["type": "text", "text": "same", "marks": [["type": "link", "attrs": ["href": href]]]]]]
        }
        let table: [String: Any] = ["type": "table", "content": [["type": "table_row", "content": [
            ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link("https://cell-one.example")]],
            ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link("https://cell-two.example")]]
        ]]]]
        let layout = try prepare(jsonSource(["type": "doc", "content": [link("https://before.example"), table, link("https://after.example")]]), configJSON: config)
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        var activated: String?
        drawing.onActivateInteraction = { activated = $0.href; return true }
        let cell = try XCTUnwrap(drawing.accessibilityElement(at: 1) as? UIAccessibilityElement)
        XCTAssertEqual(drawing.index(ofAccessibilityElement: cell), 1)
        XCTAssertTrue(cell.accessibilityActivate())
        XCTAssertEqual(activated, "https://cell-one.example")
        drawing.setTableLogicalOffset(surface.bounds.width - surface.hostViewportWidth, sourceIdentity: surface.identity)
        activated = nil
        XCTAssertFalse(cell.accessibilityActivate())
        XCTAssertNil(activated)
        drawing.linkInteractionsEnabled = false
        XCTAssertEqual((drawing.accessibilityElement(at: 1) as? UIAccessibilityElement)?.accessibilityTraits, .staticText)
    }

    func testCompilerBackedTableInteractionsAndAccessibilityRejectClippedAndStalePresentation() throws {
        let config = try interactionTableConfig()
        func link(_ href: String) -> [String: Any] {
            ["type": "paragraph", "content": [[
                "type": "text", "text": "same", "marks": [["type": "link", "attrs": ["href": href]]]
            ]]]
        }
        let nestedCell: [String: Any] = [
            "type": "table_cell",
            "attrs": ["colwidth": [300]],
            "content": [[
                "type": "paragraph",
                "content": [["type": "mention", "attrs": [
                    "label": "Nested", "id": "nested-42", "role": "clinician"
                ]]]
            ]]
        ]
        let nested: [String: Any] = [
            "type": "table",
            "content": [["type": "table_row", "content": [nestedCell]]]
        ]
        let table: [String: Any] = ["type": "table", "content": [["type": "table_row", "content": [
            ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link("https://cell-one.example")]],
            ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link("https://cell-two.example"), nested]],
            ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link("https://cell-three.example")]]
        ]]]]
        let source = try jsonSource(["type": "doc", "content": [
            link("https://before.example"), table, link("https://after.example")
        ]])
        let layout = try prepare(source, configJSON: config)
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        let window = UIWindow(frame: CGRect(origin: .zero, size: layout.size))
        window.addSubview(drawing)
        window.isHidden = false
        defer { window.isHidden = true }
        drawing.install(layout: layout)
        let nestedLayout = try XCTUnwrap(surface.cells[1].content.blocks.first { $0.tableSurface != nil }?.tableSurface?.cells.first?.content)
        let expectedMention = try XCTUnwrap(nestedLayout.interactions.first { $0.kind == .mention })

        var activated: [PreparedProseInteraction] = []
        drawing.onActivateInteraction = { interaction in
            activated.append(interaction)
            return true
        }
        func elements(named label: String) throws -> [UIAccessibilityElement] {
            try (0..<drawing.accessibilityElementCount()).compactMap { index in
                let element = try XCTUnwrap(drawing.accessibilityElement(at: index) as? UIAccessibilityElement)
                return element.accessibilityLabel == label ? element : nil
            }
        }

        let before = try XCTUnwrap(elements(named: "same").first)
        XCTAssertEqual(drawing.index(ofAccessibilityElement: before), 0)
        XCTAssertTrue(before.accessibilityActivate())
        XCTAssertEqual(activated.last?.href, "https://before.example")
        let initialSame = try elements(named: "same")
        XCTAssertGreaterThanOrEqual(initialSame.count, 4)
        XCTAssertTrue(initialSame[1].accessibilityActivate())
        XCTAssertEqual(activated.last?.href, "https://cell-one.example")
        XCTAssertTrue((try XCTUnwrap(elements(named: "same").last)).accessibilityActivate())
        XCTAssertEqual(activated.last?.href, "https://after.example")

        let initial = ViewerTablePresentation.project(layout: layout, owner: ViewerTablePresentationOwner(), viewport: .unknown)
        let firstCell = try XCTUnwrap(initial.cells.first { $0.surface === surface })
        let offset = firstCell.bounds.width
        let initialFirst = try XCTUnwrap(initial.interactions.first { $0.interaction.href == "https://cell-one.example" })
        let partialOwner = ViewerTablePresentationOwner()
        let partialOffset = initialFirst.rects[0].midX - initialFirst.clip.minX
        partialOwner.setLogicalOffset(partialOffset, for: surface)
        let partiallyShifted = ViewerTablePresentation.project(layout: layout, owner: partialOwner, viewport: .unknown)
        let partialFirst = try XCTUnwrap(partiallyShifted.interactions.first { $0.interaction.href == "https://cell-one.example" })
        let partialFrame = partialFirst.rects.reduce(CGRect.null) { $0.union($1) }.intersection(partialFirst.clip)
        XCTAssertFalse(partialFrame.isNull || partialFrame.isEmpty)
        XCTAssertLessThan(partialFrame.width, initialFirst.rects.reduce(CGRect.null) { $0.union($1) }.width)
        drawing.setTableLogicalOffset(partialOffset, sourceIdentity: surface.identity)
        let partiallyVisible = try elements(named: "same")[1]
        XCTAssertEqual(
            partiallyVisible.accessibilityFrame,
            UIAccessibility.convertToScreenCoordinates(partialFrame, in: drawing)
        )
        XCTAssertEqual(
            partiallyVisible.accessibilityPath?.bounds,
            UIAccessibility.convertToScreenCoordinates(partialFrame, in: drawing)
        )
        XCTAssertTrue(partiallyVisible.accessibilityActivate())
        XCTAssertEqual(activated.last?.href, "https://cell-one.example")

        let owner = ViewerTablePresentationOwner()
        owner.setLogicalOffset(offset, for: surface)
        let shifted = ViewerTablePresentation.project(layout: layout, owner: owner, viewport: .unknown)
        let clippedFirst = try XCTUnwrap(shifted.interactions.first { $0.interaction.href == "https://cell-one.example" })
        let exposedSecond = try XCTUnwrap(shifted.interactions.first { $0.interaction.href == "https://cell-two.example" })
        let nestedMention = try XCTUnwrap(shifted.interactions.first { $0.interaction.kind == .mention })
        XCTAssertTrue(clippedFirst.rects.allSatisfy { !$0.intersects(clippedFirst.clip) })
        XCTAssertTrue(exposedSecond.rects.contains { $0.intersects(exposedSecond.clip) })
        XCTAssertTrue(nestedMention.rects.contains { $0.intersects(nestedMention.clip) })

        drawing.setTableLogicalOffset(offset, sourceIdentity: surface.identity)
        func center(_ rect: CGRect) -> CGPoint { CGPoint(x: rect.midX, y: rect.midY) }
        XCTAssertNil(drawing.interaction(at: center(clippedFirst.rects[0])))
        XCTAssertEqual(drawing.interaction(at: center(exposedSecond.rects[0]))?.href, "https://cell-two.example")
        XCTAssertEqual(drawing.interaction(at: center(nestedMention.rects[0]))?.kind, .mention)

        let shiftedSame = try elements(named: "same")
        let clippedElement = shiftedSame[1]
        XCTAssertEqual(drawing.index(ofAccessibilityElement: clippedElement), 1)
        XCTAssertEqual(clippedElement.accessibilityFrame, .zero)
        XCTAssertNil(clippedElement.accessibilityPath)
        XCTAssertFalse(clippedElement.accessibilityActivate())

        let freshSecond = shiftedSame[2]
        XCTAssertTrue(freshSecond.accessibilityActivate())
        XCTAssertEqual(activated.last?.href, "https://cell-two.example")
        let mention = try XCTUnwrap((0..<drawing.accessibilityElementCount()).compactMap { index -> UIAccessibilityElement? in
            let element = drawing.accessibilityElement(at: index) as? UIAccessibilityElement
            return element?.accessibilityTraits.contains(.button) == true ? element : nil
        }.first)
        XCTAssertTrue(mention.accessibilityActivate())
        XCTAssertEqual(activated.last?.kind, .mention)
        XCTAssertEqual(activated.last?.label, "Nested")
        XCTAssertEqual(activated.last?.docPos, expectedMention.docPos)
        XCTAssertEqual(activated.last?.attrsJSON, expectedMention.attrsJSON)

        drawing.linkInteractionsEnabled = false
        XCTAssertEqual(drawing.index(ofAccessibilityElement: freshSecond), NSNotFound)
        let callbacksBeforeStaleCapabilityActivation = activated.count
        XCTAssertFalse(freshSecond.accessibilityActivate())
        XCTAssertEqual(activated.count, callbacksBeforeStaleCapabilityActivation)
        XCTAssertEqual((try XCTUnwrap(elements(named: "same").first)).accessibilityTraits, .staticText)
        let enabledMention = try XCTUnwrap((0..<drawing.accessibilityElementCount()).compactMap { index -> UIAccessibilityElement? in
            let element = drawing.accessibilityElement(at: index) as? UIAccessibilityElement
            return element?.accessibilityTraits.contains(.button) == true ? element : nil
        }.first)
        XCTAssertTrue(enabledMention.accessibilityActivate())
        XCTAssertEqual(activated.last?.kind, .mention)

        drawing.linkInteractionsEnabled = true
        let staleRoot = try XCTUnwrap(elements(named: "same").first)
        drawing.install(layout: try prepare(source, configJSON: config))
        XCTAssertEqual(drawing.index(ofAccessibilityElement: staleRoot), NSNotFound)
        let callbacksBeforeStaleRootActivation = activated.count
        XCTAssertFalse(staleRoot.accessibilityActivate())
        XCTAssertEqual(activated.count, callbacksBeforeStaleRootActivation)
    }
    func testCompilerBackedGlobalImageAdmissionCountsFlatTableCellsBeforeLayoutPreparation() throws {
        let admittedCounter = PreparationCounter()
        let admittedRegistry = admissionRegistry(counter: admittedCounter)
        let admittedRequest = ProseViewerRequest(
            source: .json(try imageTableSource(imageCount: ViewerImageAttachment.maximumAdmittedAttachments)),
            configuration: ProseViewerConfiguration(configJSON: Self.config, imagesEnabled: true)
        )

        let admitted = admittedRegistry.measure(request: admittedRequest, widthPoints: 320, scale: 2)
        XCTAssertNil(admitted.error)
        XCTAssertEqual(admittedCounter.value, 1)

        let rejectedCounter = PreparationCounter()
        let rejectedRegistry = admissionRegistry(counter: rejectedCounter)
        let rejectedRequest = ProseViewerRequest(
            source: .json(try imageTableSource(imageCount: ViewerImageAttachment.maximumAdmittedAttachments + 1)),
            configuration: ProseViewerConfiguration(configJSON: Self.config, imagesEnabled: true)
        )

        let rejected = rejectedRegistry.measure(request: rejectedRequest, widthPoints: 320, scale: 2)
        XCTAssertEqual(rejected.error?.code, "ATTACHMENT_LIMIT_EXCEEDED")
        XCTAssertEqual(rejectedCounter.value, 0)
    }

    func testCompilerBackedRaisedDepth110TablesPrepareFiniteRetainedSurfaces() throws {
        let layout = try prepare(
            nestedTablesSource(depth: 110),
            configJSON: try configWithMaxDocumentDepth(1024)
        )

        XCTAssertNil(layout.error)
        XCTAssertTrue(layout.size.width.isFinite && layout.size.height.isFinite)
        var prepared = layout
        var surfaceCount = 0
        while let surface = prepared.blocks.first?.tableSurface {
            surfaceCount += 1
            XCTAssertTrue(surface.layout.contentSize.width.isFinite && surface.layout.contentSize.height.isFinite)
            XCTAssertEqual(surface.cells.count, 1)
            prepared = try XCTUnwrap(surface.cells.first?.content)
        }
        XCTAssertEqual(surfaceCount, 110)
    }

    func testCompilerBackedIdenticalAtomCellsMeasureEachSourceHeightIntoTheSharedRow() throws {
        let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"card"}]},{"type":"table_cell","attrs":{"colwidth":[100]},"content":[{"type":"card"}]}]}]}]}"#
        var result = viewerCompile(request: FfiViewerCompileRequest(sourceKind: .json, source: source, configJson: Self.config, imagesEnabled: false, mentionPrefix: nil))
        if let error = result.error {
            throw ProseViewerError.compiler(domain: error.domain, code: error.code, message: error.message)
        }
        let document = try ViewerDocument(compiled: try XCTUnwrap(result.value))
        result.value = nil
        let table = try XCTUnwrap(document.blocks.first?.table)
        let atomPositions = try table.cells.map { cell -> UInt32 in
            let child = try document.cellDocument(for: cell)
            let inline = try XCTUnwrap(child.blocks.first?.inlines.first)
            guard case let .atom(_, docPos, _, _) = inline else {
                throw ProseViewerError.layout(message: "Expected a compiler-lowered block atom.")
            }
            return docPos
        }

        XCTAssertEqual(Set(table.cells.map(\.contentKey)).count, 1)
        XCTAssertEqual(Set(atomPositions).count, 2)

        let themeJSON = """
        {"viewerAtoms":{"generation":"table-atoms","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":40},"measurements":{"\(atomPositions[0])":{"width":82,"height":20},"\(atomPositions[1])":{"width":82,"height":100}}}}
        """
        let theme = PreparedProseTheme.resolve(themeJSON: themeJSON)
        let key = ProseLayoutKey(semanticKey: document.semanticKey, widthPixels: 640, themeDigest: "table-atoms", nativeFontRevision: 0, fontEnvironmentRevision: 0, displayScale: 2, attachmentRevision: 0, generationIdentity: "table-atoms", semanticGenerationIdentity: "table-atoms")
        let layout = try CoreTextProseLayoutEngine().prepare(document: document.withPreparedTheme(theme), key: key, widthPoints: 320, displayScale: 2)
        let surface = try XCTUnwrap(layout.blocks.first?.tableSurface)
        let chrome = 2 * (TableStyle().cellPadding + TableStyle().borderWidth)
        let atomHeights = try surface.cells.map { try XCTUnwrap($0.content.blocks.first?.atomSlot).bounds.height }

        XCTAssertEqual(atomHeights, [20, 100])
        for cell in surface.cells {
            XCTAssertGreaterThanOrEqual(cell.frame.height, cell.content.size.height + chrome)
        }
        let tallCell = try XCTUnwrap(surface.cells.first(where: { $0.content.size.height == 100 }))
        XCTAssertGreaterThanOrEqual(tallCell.frame.height, 100 + chrome)
    }

    func testCompilerBackedCellsKeepRichBlocksAndNestedImagesInParentOrder() throws {
        let source = #"{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.test/outer.png","width":20,"height":10}},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"bold","marks":[{"type":"bold"}]}]},{"type":"image","attrs":{"src":"https://example.test/cell.png","width":20,"height":10}},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"image","attrs":{"src":"https://example.test/nested.png","width":20,"height":10}}]}]}]},{"type":"image","attrs":{"src":"https://example.test/after.png","width":20,"height":10}}]}]}]}]}"#
        let layout = try prepare(source)

        let table = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(table.tableSurface)
        XCTAssertEqual(surface.cells.count, 1)
        XCTAssertEqual(surface.cells[0].content.key.widthPixels, Int((surface.cells[0].content.size.width * 2).rounded()))
        XCTAssertGreaterThanOrEqual(surface.cells[0].content.blocks.count, 2)
        let boldLine = try XCTUnwrap(surface.cells[0].content.blocks.flatMap(\.fragments).first { $0.kind == .text }?.line)
        let boldRun = try XCTUnwrap((CTLineGetGlyphRuns(boldLine) as? [CTRun])?.first)
        let attributes = try XCTUnwrap(CTRunGetAttributes(boldRun) as? [NSAttributedString.Key: Any])
        let font = attributes[kCTFontAttributeName as NSAttributedString.Key] as! CTFont
        XCTAssertTrue(CTFontGetSymbolicTraits(font).contains(.traitBold))
        XCTAssertEqual(surface.layout.contentSize.height, surface.cells[0].content.size.height + 2 * (TableStyle().cellPadding + TableStyle().borderWidth), accuracy: 0.01)
        XCTAssertEqual(layout.imageAttachments.map(\.ordinal), [0, 1, 2, 3])
        XCTAssertEqual(layout.imageAttachments.map(\.source), [
            "https://example.test/outer.png", "https://example.test/cell.png", "https://example.test/nested.png", "https://example.test/after.png"
        ], "Root source IDs: \(layout.imageAttachments.map(\.id))")
        XCTAssertTrue(layout.imageAttachments.allSatisfy { $0.id.contains($0.source) })
        XCTAssertTrue(layout.imageAttachments.allSatisfy { finite($0.bounds) })
        let localCellImage = try XCTUnwrap(surface.cells[0].content.imageAttachments.first)
        let parentCellImage = try XCTUnwrap(layout.imageAttachments.first { $0.source == "https://example.test/cell.png" })
        XCTAssertEqual(parentCellImage.bounds.minX, table.bounds.minX + surface.cells[0].frame.minX + surface.cells[0].contentOrigin.x + localCellImage.bounds.minX, accuracy: 0.01)
        XCTAssertEqual(parentCellImage.bounds.minY, table.bounds.minY + surface.cells[0].frame.minY + surface.cells[0].contentOrigin.y + localCellImage.bounds.minY, accuracy: 0.01)
        let nestedSurface = try XCTUnwrap(surface.cells[0].content.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(
            surface.cells[0].content.imageAttachments.map(\.source),
            ["https://example.test/cell.png", "https://example.test/after.png"],
            "Outer cell source IDs: \(surface.cells[0].content.imageAttachments.map(\.id))"
        )
        XCTAssertEqual(
            nestedSurface.cells[0].content.imageAttachments.map(\.source),
            ["https://example.test/nested.png"],
            "Nested cell source IDs: \(nestedSurface.cells[0].content.imageAttachments.map(\.id))"
        )
        let retainedAttachmentRecords = layout.imageAttachments.count + surface.cells[0].content.imageAttachments.count
            + nestedSurface.cells[0].content.imageAttachments.count
        XCTAssertEqual(retainedAttachmentRecords, 7)
    }

    func testCompilerBackedIrregularGeometryIsFiniteAndRetainsEverySourceCell() throws {
        let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"tall"}]}]},{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"wide"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"later"}]}]}]}]}]}"#
        let layout = try prepare(source)
        let surface = try XCTUnwrap(layout.blocks.first?.tableSurface)
        XCTAssertNil(surface.layout.failure)
        XCTAssertEqual(surface.cells.map(\.sourcePosition), surface.layout.sourceOrder)
        XCTAssertEqual(surface.cells.count, 3)
        XCTAssertTrue(surface.layout.contentSize.width.isFinite && surface.layout.contentSize.height.isFinite)
        XCTAssertTrue(surface.layout.rectangles.values.allSatisfy(finite))
    }

    func testCompilerAdmissionRejectsOverlimitGridAndTypedFallbackPaintsWithAdjacentProse() throws {
        let source = gridLimitSource()
        var rejected = viewerCompile(request: FfiViewerCompileRequest(
            sourceKind: .json,
            source: source,
            configJson: try configWithGridSlots(1),
            imagesEnabled: true,
            mentionPrefix: nil
        ))
        XCTAssertNil(rejected.value)
        XCTAssertEqual(rejected.error?.code, "DOCUMENT_LIMIT_EXCEEDED")

        var compiled = viewerCompile(request: FfiViewerCompileRequest(
            sourceKind: .json,
            source: source,
            configJson: Self.config,
            imagesEnabled: true,
            mentionPrefix: nil
        ))
        if let error = compiled.error {
            throw ProseViewerError.compiler(domain: error.domain, code: error.code, message: error.message)
        }
        let document = try ViewerDocument(compiled: try XCTUnwrap(compiled.value))
        compiled.value = nil
        let sourceBlocks = document.blocks.filter { $0.table != nil }
        XCTAssertEqual(sourceBlocks.count, 1)
        let sourceTable = try XCTUnwrap(sourceBlocks.first?.table)
        XCTAssertFalse(sourceTable.cells.isEmpty)
        XCTAssertFalse(sourceTable.sourceRows.isEmpty)
        let failedTable = FfiViewerTable(
            tablePos: sourceTable.tablePos,
            sourceEnd: sourceTable.sourceEnd,
            rows: 0,
            columns: 0,
            columnWidths: [],
            direction: sourceTable.direction,
            irregular: true,
            readOnlyDescendants: sourceTable.readOnlyDescendants,
            attrsKey: sourceTable.attrsKey,
            sourceRows: [],
            cells: [],
            syntheticRegions: [],
            failure: .gridLimit,
            compatibilityDiagnostic: nil
        )
        let blocks = document.blocks.map { block -> ViewerBlock in
            guard block.table?.tablePos == sourceTable.tablePos else { return block }
            return ViewerBlock(
                nodeType: block.nodeType,
                depth: block.depth,
                inBlockquote: block.inBlockquote,
                listContext: block.listContext,
                listItemBoundary: block.listItemBoundary,
                listItemAncestors: block.listItemAncestors,
                outermostListItemIdentity: block.outermostListItemIdentity,
                outermostListItemIsLast: block.outermostListItemIsLast,
                inlines: block.inlines,
                isBlockAtom: block.isBlockAtom,
                styleAncestors: block.styleAncestors,
                language: block.language,
                table: failedTable
            )
        }
        let failureDocument = ViewerDocument(
            semanticKey: document.semanticKey,
            blocks: blocks,
            isEmpty: document.isEmpty,
            retainedBytes: document.retainedBytes,
            trailingEmptyTextBlockCount: document.trailingEmptyTextBlockCount,
            tableAttributes: document.tableAttributes,
            tableRecords: document.tableRecords.merging(["t\(sourceTable.tablePos)": failedTable]) { _, replacement in replacement },
            preferredTextBlockName: document.preferredTextBlockName
        )
        let layout = try prepare(failureDocument)
        let table = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(table.tableSurface)
        XCTAssertEqual(surface.layout.failure, .gridLimit)
        XCTAssertNil(surface.preparationError)
        XCTAssertTrue(surface.cells.isEmpty)
        XCTAssertTrue(surface.layout.sourceOrder.isEmpty)
        XCTAssertTrue(layout.interactions.isEmpty)
        let frame = try XCTUnwrap(table.tableBounds)
        XCTAssertTrue(finite(frame))
        XCTAssertGreaterThan(frame.width, 0)
        XCTAssertGreaterThan(frame.height, 0)
        XCTAssertGreaterThanOrEqual(frame.minX, 0)
        XCTAssertGreaterThanOrEqual(frame.minY, 0)
        XCTAssertLessThanOrEqual(frame.maxX, layout.size.width)
        XCTAssertLessThanOrEqual(frame.maxY, layout.size.height)

        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        let rendered = try XCTUnwrap(UIGraphicsImageRenderer(size: layout.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }.cgImage)
        XCTAssertTrue(
            (Int(frame.minY)..<Int(frame.maxY)).contains { y in
                (Int(frame.minX)..<Int(frame.maxX)).contains { x in
                    let pixel = try! rgba(rendered, CGPoint(x: x, y: y))
                    return pixel[3] > 0 && pixel[0] > 200 && pixel[1] < 100 && pixel[2] < 100
                }
            },
            "typed fallback must draw its red failure frame"
        )
        let prose = layout.blocks.filter { $0.tableSurface == nil }
        XCTAssertEqual(prose.count, 2)
        XCTAssertTrue(
            prose.allSatisfy { block in
                guard let bounds = block.fragments.first(where: { $0.kind == .text })?.bounds else { return false }
                return (Int(bounds.minY)..<Int(bounds.maxY)).contains { y in
                    (Int(bounds.minX)..<Int(bounds.maxX)).contains { x in
                        let pixel = try! rgba(rendered, CGPoint(x: x, y: y))
                        return pixel[3] > 0 && pixel[0] < 200 && pixel[1] < 200 && pixel[2] < 200
                    }
                }
            },
            "compiler-prepared prose before and after the fallback must retain ink"
        )
    }

    func testCompilerBackedIdenticalCellsKeepDistinctPreparedArtifacts() throws {
        let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"same"}]}]}]}]}]}"#
        let layout = try prepare(source)
        let cells = try XCTUnwrap(layout.blocks.first?.tableSurface).cells
        XCTAssertEqual(cells.count, 2)
        XCTAssertNotEqual(cells[0].sourcePosition, cells[1].sourcePosition)
        XCTAssertNotEqual(cells[0].content.key.semanticKey, cells[1].content.key.semanticKey)
    }

    func testCellModeSuppressesOnlyCellContentBoxAndOuterContainersEncloseTable() throws {
        let source = #"{"type":"doc","content":[{"type":"blockquote","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"inside"}]}]}]}]}]}]}"#
        let layout = try prepare(source, themeJSON: ##"{"version":1,"styles":{"content":{"paddingLeft":20,"paddingRight":20,"backgroundColor":"#ff0000ff"},"blockquote":{"paddingLeft":11,"paddingRight":13,"backgroundColor":"#ffff00ff"}}}"##)
        let table = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(table.tableSurface)
        let red = UIColor.red
        let yellow = UIColor.yellow
        XCTAssertEqual(layout.decorations.filter { $0.styleBox?.color("backgroundColor") == red }.count, 1)
        XCTAssertFalse(surface.cells.flatMap { $0.content.decorations }.contains { $0.styleBox?.color("backgroundColor") == red })
        let quote = try XCTUnwrap(layout.decorations.first { $0.styleBox?.color("backgroundColor") == yellow })
        XCTAssertTrue(quote.bounds.contains(table.bounds))
        XCTAssertGreaterThan(table.bounds.minX, 20)
    }

    func testCompilerBackedTableCodeHighlightingUsesCanonicalSourceScopes() throws {
        let source = try jsonSource([
            "type": "doc",
            "content": [
                codeBlock("root"),
                ["type": "table", "content": [
                    ["type": "table_row", "content": [
                        ["type": "table_cell", "attrs": ["rowspan": 2], "content": [codeBlock("cell")]],
                        ["type": "table_cell", "content": [
                            ["type": "table", "content": [
                                ["type": "table_row", "content": [
                                    ["type": "table_cell", "content": [codeBlock("nested")]]
                                ]]
                            ]]
                        ]]
                    ]],
                    ["type": "table_row", "content": [
                        ["type": "table_cell", "content": [paragraph("later")]]
                    ]]
                ]]
            ]
        ])
        let generation = "table-highlight-\(UUID().uuidString)"
        let initial = try prepareHighlighted(source, generation: generation)
        let request = try XCTUnwrap(initial.highlightingRequest)
        XCTAssertEqual(request.blocks.map(\.text), ["root", "cell", "nested"])
        XCTAssertEqual(request.blocks.map(\.start), [0, 1, 2])
        XCTAssertEqual(Set(request.blocks.map(\.start)).count, request.blocks.count)

        let outerSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let outerRecord = TableGridRecord(table: try XCTUnwrap(outerSurface.sourceTable), documentOwner: "test")
        XCTAssertEqual(outerRecord.rows, 2)
        XCTAssertEqual(try XCTUnwrap(outerRecord.cells.first).rowspan, 2)
        let cell = try XCTUnwrap(outerSurface.cells.first { $0.content.blocks.contains { $0.fragments.contains { $0.kind == .text } } })
        let nestedSurface = try XCTUnwrap(outerSurface.cells.first { $0.content.blocks.contains { $0.tableSurface != nil } }?.content.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertNil(cell.content.highlightingRequest)
        XCTAssertNil(nestedSurface.cells[0].content.highlightingRequest)

        let colors: [UInt32] = [0xff0000ff, 0x00ff00ff, 0x0000ffff]
        let output = zip(request.blocks, colors).map { block, color in
            NativeHighlightedCodeBlock(
                block: block,
                ranges: [NativeCodeHighlightRange(start: 0, length: block.text.utf16.count, color: color, fontStyle: 0)]
            )
        }
        PreparedViewerHighlightingStore.publish(PreparedViewerHighlightingResult(output), generation: generation)

        let resolved = try prepareHighlighted(source, generation: generation)
        let resolvedSurface = try XCTUnwrap(resolved.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let resolvedCell = try XCTUnwrap(resolvedSurface.cells.first { $0.content.blocks.contains { $0.fragments.contains { $0.kind == .text } } })
        let resolvedNested = try XCTUnwrap(resolvedSurface.cells.first { $0.content.blocks.contains { $0.tableSurface != nil } }?.content.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertTrue(resolved.highlightingResolved)
        XCTAssertNil(resolvedCell.content.highlightingRequest)
        XCTAssertNil(resolvedNested.cells[0].content.highlightingRequest)
        XCTAssertEqual(highlightColor(in: resolved), UIColor.red.cgColor)
        XCTAssertEqual(highlightColor(in: resolvedCell.content), UIColor.green.cgColor)
        XCTAssertEqual(highlightColor(in: resolvedNested.cells[0].content), UIColor.blue.cgColor)
    }

    func testCompilerBackedTableOnlyListItemReservesOneMarkerGutter() throws {
        let source = try jsonSource([
            "type": "doc",
            "content": [["type": "bulletList", "content": [
                ["type": "listItem", "content": [["type": "table", "content": [
                    ["type": "table_row", "content": [
                        ["type": "table_cell", "content": [paragraph("table")]]
                    ]]
                ]]]],
                ["type": "listItem", "content": [paragraph("following")]]
            ]]]
        ])
        let layout = try prepare(source)
        let table = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        XCTAssertEqual(table.fragments.filter { $0.kind == .marker }.count, 1)
        XCTAssertGreaterThan(table.bounds.minX, 0)
        XCTAssertLessThan(table.bounds.minY, try XCTUnwrap(layout.blocks.last?.bounds.minY))
    }

    func testTableOnlyListItemsUseOneMarkerAndTerminalSpacingPerItem() throws {
        let source = try jsonSource(["type": "doc", "content": [["type": "bulletList", "content": [
            ["type": "listItem", "content": [["type": "table", "content": [["type": "table_row", "content": [["type": "table_cell", "content": [paragraph("first")]]]]]]]],
            ["type": "listItem", "content": [["type": "table", "content": [["type": "table_row", "content": [["type": "table_cell", "content": [paragraph("final")]]]]]]]]
        ]]]])
        let layout = try prepare(source)
        let tables = layout.blocks.filter { $0.tableSurface != nil }
        XCTAssertEqual(tables.count, 2)
        XCTAssertTrue(tables.allSatisfy { $0.fragments.filter { $0.kind == .marker }.count == 1 })
        XCTAssertGreaterThan(tables[1].bounds.minY, tables[0].bounds.maxY)
    }

    func testNestedTableOnlyListItemKeepsBothAncestorGutters() throws {
        let table: [String: Any] = ["type": "table", "content": [["type": "table_row", "content": [["type": "table_cell", "content": [paragraph("nested")]]]]]]
        let source = try jsonSource(["type": "doc", "content": [
            ["type": "bulletList", "content": [["type": "listItem", "content": [["type": "bulletList", "content": [["type": "listItem", "content": [table]]]]]]]],
            paragraph("after")
        ]])
        let layout = try prepare(source, themeJSON: #"{"list":{"itemSpacing":3,"spacingAfter":13}}"#)
        let tableBlock = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        XCTAssertEqual(tableBlock.fragments.filter { $0.kind == .marker }.count, 1)
        XCTAssertGreaterThan(tableBlock.bounds.minX, 20)
        let after = try XCTUnwrap(layout.blocks.last(where: { $0.tableSurface == nil }))
        XCTAssertEqual(after.bounds.minY - tableBlock.tableBounds!.maxY, 26, accuracy: 0.01)
    }

    func testTableOnlyListItemsUseDistinctFinalAndNonfinalSpacing() throws {
        func table(_ text: String) -> [String: Any] {
            ["type": "table", "content": [["type": "table_row", "content": [["type": "table_cell", "content": [paragraph(text)]]]]]]
        }
        let source = try jsonSource(["type": "doc", "content": [
            ["type": "bulletList", "content": [
                ["type": "listItem", "content": [table("first")]],
                ["type": "listItem", "content": [table("final")]]
            ]],
            paragraph("after")
        ]])
        let layout = try prepare(source, themeJSON: #"{"list":{"itemSpacing":3,"spacingAfter":13}}"#)
        let tables = layout.blocks.filter { $0.tableSurface != nil }
        XCTAssertEqual(tables.count, 2)
        XCTAssertEqual(tables[1].tableBounds!.minY - tables[0].tableBounds!.maxY, 3, accuracy: 0.01)
        let after = try XCTUnwrap(layout.blocks.last(where: { $0.tableSurface == nil }))
        XCTAssertEqual(after.bounds.minY - tables[1].tableBounds!.maxY, 13, accuracy: 0.01)
    }

    func testCompilerBackedMountedPresentationKeepsFullMetadataAndBoundsOffsetsIndependently() throws {
        let rtlConfig = Self.config.replacingOccurrences(
            of: "\"class\":{\"default\":null}",
            with: "\"class\":{\"default\":null},\"dir\":{\"default\":null}"
        ).replacingOccurrences(
            of: "\"marks\":[{\"name\":\"bold\"}]",
            with: "\"marks\":[{\"name\":\"bold\"},{\"name\":\"link\",\"attrs\":{\"href\":{\"default\":\"\"}}}]"
        )
        func cell(_ content: [[String: Any]]) -> [String: Any] {
            ["type": "table_cell", "attrs": ["colwidth": [100]], "content": content]
        }
        let image = ["type": "image", "attrs": ["src": "https://example.test/first.png", "width": 20, "height": 10]] as [String: Any]
        let nestedImage = ["type": "image", "attrs": ["src": "https://example.test/nested.png", "width": 20, "height": 10]] as [String: Any]
        func linked(_ label: String) -> [String: Any] {
            ["type": "paragraph", "content": [["type": "text", "text": label, "marks": [["type": "link", "attrs": ["href": "https://example.test/\(label)"]]]]]]
        }
        let nested = ["type": "table", "content": [["type": "table_row", "content": [cell([nestedImage])]]] as [[String: Any]]] as [String: Any]
        let tableSource = ["type": "table", "attrs": ["dir": "rtl"], "content": [["type": "table_row", "content": [cell([image, ["type": "card"], linked("cell")]), cell([nested]), cell([paragraph("three")]), cell([paragraph("four")])]]] as [[String: Any]]] as [String: Any]
        let source = try jsonSource(["type": "doc", "content": [linked("before"), ["type": "bulletList", "content": [["type": "listItem", "content": [tableSource]]]], linked("after")]])
        let layout = try prepare(source, themeJSON: #"{"viewerAtoms":{"generation":"presentation","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":20}}}"#, configJSON: rtlConfig)
        let table = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil })
        let surface = try XCTUnwrap(table.tableSurface)
        guard case .rightToLeft = surface.direction else {
            return XCTFail("Expected the declared RTL direction.")
        }
        XCTAssertLessThan(surface.hostViewportWidth, surface.bounds.width)

        let firstOwner = ViewerTablePresentationOwner()
        let full = ViewerTablePresentation.project(layout: layout, owner: firstOwner, viewport: .unknown)
        XCTAssertEqual(full.cells.filter { $0.surface === surface }.map(\.sourcePosition), surface.layout.sourceOrder)
        XCTAssertEqual(full.mountedCells.filter { $0.surface === surface }.count, surface.cells.count)
        XCTAssertEqual(full.images.count, 2)
        XCTAssertEqual(full.atoms.count, 1)
        XCTAssertEqual(full.interactions.map(\.interaction.visibleText), ["before", "cell", "after"])
        XCTAssertTrue(full.accessibilityNodes.contains { $0.interactionSourceIdentity == full.interactions.first?.sourceIdentity })
        let first = try XCTUnwrap(full.cells.first)
        XCTAssertEqual(first.bounds.maxX, first.clip.maxX, accuracy: 0.01)
        let nestedCell = try XCTUnwrap(full.cells.first { $0.surface !== surface })
        let containingCell = try XCTUnwrap(full.cells.first { $0.surface === surface && $0.sourcePosition == surface.layout.sourceOrder[1] })
        let nestedBlock = try XCTUnwrap(full.blocks.first { $0.layout === containingCell.content && $0.block.tableSurface != nil })
        let nestedSurface = try XCTUnwrap(nestedBlock.block.tableSurface)
        let nestedFrame = try XCTUnwrap(nestedBlock.block.tableBounds)
        let nestedOrigin = CGPoint(x: containingCell.contentBounds.minX + nestedFrame.minX, y: containingCell.contentBounds.minY + nestedFrame.minY)
        let expectedNestedClip = containingCell.clip.intersection(containingCell.contentBounds).intersection(
            CGRect(origin: nestedOrigin, size: CGSize(width: min(nestedSurface.hostViewportWidth, nestedSurface.bounds.width), height: nestedSurface.bounds.height))
        )
        XCTAssertEqual(nestedCell.clip, expectedNestedClip)

        let secondOwner = ViewerTablePresentationOwner()
        secondOwner.setLogicalOffset(.infinity, for: surface)
        XCTAssertEqual(secondOwner.logicalOffset(for: surface), 0, accuracy: 0.01)
        secondOwner.setLogicalOffset(.greatestFiniteMagnitude, for: surface)
        let shifted = ViewerTablePresentation.project(layout: layout, owner: secondOwner, viewport: .unknown)
        let shiftedFirst = try XCTUnwrap(shifted.cells.first)
        XCTAssertEqual(shiftedFirst.bounds.maxX - first.bounds.maxX, surface.bounds.width - surface.hostViewportWidth, accuracy: 0.01)
        XCTAssertEqual(firstOwner.logicalOffset(for: surface), 0, accuracy: 0.01)
        XCTAssertEqual(shifted.images.map(\.sourceIdentity), full.images.map(\.sourceIdentity))
        XCTAssertEqual(shifted.images.map(\.bounds.size), full.images.map(\.bounds.size))
        XCTAssertEqual(shifted.atoms.map(\.sourceIdentity), full.atoms.map(\.sourceIdentity))
        XCTAssertEqual(shifted.atoms.map(\.bounds.size), full.atoms.map(\.bounds.size))
        XCTAssertEqual(shifted.interactions.map(\.sourceIdentity), full.interactions.map(\.sourceIdentity))
        let displacement = shiftedFirst.bounds.maxX - first.bounds.maxX
        for (before, after) in zip(full.images, shifted.images) { XCTAssertEqual(after.bounds.minX - before.bounds.minX, displacement, accuracy: 0.01) }
        for (before, after) in zip(full.atoms, shifted.atoms) { XCTAssertEqual(after.bounds.minX - before.bounds.minX, displacement, accuracy: 0.01) }
        let beforeCellLink = try XCTUnwrap(full.interactions.first { $0.interaction.visibleText == "cell" })
        let afterCellLink = try XCTUnwrap(shifted.interactions.first { $0.sourceIdentity == beforeCellLink.sourceIdentity })
        XCTAssertEqual(afterCellLink.rects[0].minX - beforeCellLink.rects[0].minX, displacement, accuracy: 0.01)

        let known = ViewerTablePresentation.project(
            layout: layout,
            owner: secondOwner,
            viewport: .known(CGRect(x: table.tableBounds!.minX, y: table.tableBounds!.minY, width: 100, height: 100))
        )
        XCTAssertEqual(known.mountedCells.filter { $0.surface === surface }.map(\.sourcePosition), Array(surface.layout.sourceOrder.suffix(2)))
        XCTAssertEqual(
            ViewerTablePresentation.project(layout: layout, owner: secondOwner, viewport: .known(.zero)).mountedCells.count,
            0
        )
        XCTAssertEqual(ViewerTablePresentation.project(layout: layout, owner: secondOwner, viewport: .known(CGRect(x: 100_000, y: 100_000, width: 20, height: 20))).mountedCells.count, 0)

        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        let serializedAtoms = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: Data(drawing.atomLayoutsJSON(origin: .zero).utf8)) as? [[String: Any]]
        )
        let serialized = try XCTUnwrap(serializedAtoms.first)
        let projectedAtom = try XCTUnwrap(full.atoms.first)
        XCTAssertEqual((serialized["docPos"] as? NSNumber)?.uint32Value, projectedAtom.atom.docPos)
        let serializedWidth = CGFloat(try XCTUnwrap(serialized["width"] as? NSNumber).doubleValue)
        let serializedHeight = CGFloat(try XCTUnwrap(serialized["height"] as? NSNumber).doubleValue)
        XCTAssertEqual(serializedWidth, projectedAtom.bounds.width, accuracy: 0.01)
        XCTAssertEqual(serializedHeight, projectedAtom.bounds.height, accuracy: 0.01)
        let presentation = try XCTUnwrap(serialized["presentation"] as? [String: Any])
        XCTAssertEqual(presentation["candidate"] as? Bool, true)
        let clip = try XCTUnwrap(presentation["clip"] as? [String: Any])
        let clipX = CGFloat(try XCTUnwrap(clip["x"] as? NSNumber).doubleValue)
        XCTAssertEqual(clipX, projectedAtom.clip.minX, accuracy: 0.01)
        drawing.setTableLogicalOffset(surface.bounds.width, sourceIdentity: surface.identity)
        let shiftedAtoms = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: Data(drawing.atomLayoutsJSON(origin: .zero).utf8)) as? [[String: Any]]
        )
        let shiftedRecord = try XCTUnwrap(shiftedAtoms.first)
        let shiftedWidth = CGFloat(try XCTUnwrap(shiftedRecord["width"] as? NSNumber).doubleValue)
        XCTAssertEqual(serializedWidth, shiftedWidth, accuracy: 0.01)
        XCTAssertNotEqual(serialized["x"] as? NSNumber, shiftedRecord["x"] as? NSNumber)

        let vertical = try prepare(#"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"four"}]}]}]}]}]}"#)
        let verticalBlock = try XCTUnwrap(vertical.blocks.first { $0.tableSurface != nil })
        let verticalSurface = try XCTUnwrap(verticalBlock.tableSurface)
        let middle = verticalSurface.cells[1]
        let verticalWindow = CGRect(x: verticalBlock.tableBounds!.minX, y: verticalBlock.tableBounds!.minY + middle.frame.minY, width: 20, height: middle.frame.height)
        let verticalSnapshot = ViewerTablePresentation.project(layout: vertical, owner: ViewerTablePresentationOwner(), viewport: .known(verticalWindow))
        XCTAssertEqual(verticalSnapshot.mountedCells.map(\.sourcePosition), Array(verticalSurface.layout.sourceOrder.prefix(3)))
    }

    func testCompilerBackedMountedPresentationTraversesAdmittedDepthWithoutDroppingMetadata() throws {
        let layout = try prepare(nestedTablesSource(depth: 110), configJSON: try configWithMaxDocumentDepth(1024))
        let snapshot = ViewerTablePresentation.project(
            layout: layout,
            owner: ViewerTablePresentationOwner(),
            viewport: .unknown
        )
        XCTAssertEqual(snapshot.cells.count, 110)
        XCTAssertEqual(snapshot.mountedCells.count, 110)
    }

    func testMountedTableOffsetsIncreaseAndResetFabricAndDirectHostRetention() throws {
        let source = try nestedHeaderImageSource(nestedOverflow: true)
        let layout = try prepare(source)
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertGreaterThan(surface.bounds.width, surface.hostViewportWidth)

        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        let drawingBeforeScroll = drawing.preparedSurfaceRetainedBytesForTesting
        drawing.setTableLogicalOffset(surface.bounds.width - surface.hostViewportWidth, sourceIdentity: surface.identity)
        XCTAssertGreaterThan(
            drawing.preparedSurfaceRetainedBytesForTesting,
            drawingBeforeScroll,
            "a scrolled table's mutable offset map must be included in the Fabric host total"
        )
        let drawingAfterScroll = drawing.preparedSurfaceRetainedBytesForTesting
        drawing.imagePixels = ["retained-pixel": UIImage()]
        XCTAssertEqual(
            drawing.preparedSurfaceRetainedBytesForTesting,
            drawingAfterScroll + PreparedProseImagePixelMapAccounting.retainedBytes(entryCount: 1),
            "table offset accounting must remain additive with the existing decoded-pixel map cost"
        )
        drawing.imagePixels = [:]

        let replacement = try prepare(try nestedHeaderImageSource(imageSource: "https://example.test/replacement.png", nestedOverflow: true))
        drawing.install(layout: replacement)
        XCTAssertEqual(
            drawing.tablePresentationRetainedBytesForTesting,
            0,
            "replacing the immutable artifact must drop its mounted table offset sidecar"
        )

        let engine = CoreTextProseLayoutEngine()
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let viewer = ProseViewerView(layoutRegistry: registry)
        XCTAssertTrue(viewer.apply(source: .json(source), configuration: ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true)))
        _ = viewer.sizeThatFits(CGSize(width: 390, height: CGFloat.greatestFiniteMagnitude))
        let directLayout = try XCTUnwrap(viewer.drawingViewForTesting.layout)
        let directSurface = try XCTUnwrap(directLayout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertGreaterThan(directSurface.bounds.width, directSurface.hostViewportWidth)
        let directBeforeScroll = viewer.preparedSurfaceRetainedBytesForTesting
        viewer.drawingViewForTesting.setTableLogicalOffset(
            directSurface.bounds.width - directSurface.hostViewportWidth,
            sourceIdentity: directSurface.identity
        )
        XCTAssertGreaterThan(
            viewer.preparedSurfaceRetainedBytesForTesting,
            directBeforeScroll,
            "the direct host total must include its drawing-owned table offsets"
        )
        viewer.prepareForReuse()
        XCTAssertEqual(viewer.drawingViewForTesting.tablePresentationRetainedBytesForTesting, 0)
        XCTAssertLessThan(viewer.preparedSurfaceRetainedBytesForTesting, directBeforeScroll)
    }

    func testCompilerBackedWidth390TableMeasurementsReuseRecursiveCellArtifacts() throws {
        let engine = CoreTextProseLayoutEngine()
        var preparedCellPositions: [Int] = []
        engine.tableCellPreparationObserver = { preparedCellPositions.append($0) }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let request = ProseViewerRequest(
            source: .json(try nestedHeaderImageSource(nestedOverflow: true)),
            configuration: ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true)
        )

        let first = registry.measure(request: request, widthPoints: 390, scale: 1)
        XCTAssertNil(first.error)
        let parentIdentity = ObjectIdentifier(first)
        let rootSurface = try XCTUnwrap(first.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let rootCellIdentities = rootSurface.cells.map { ObjectIdentifier($0.content) }
        let snapshot = ViewerTablePresentation.project(
            layout: first,
            owner: ViewerTablePresentationOwner(),
            viewport: .unknown
        )
        let nestedSurface = try XCTUnwrap(snapshot.cells.first { $0.surface !== rootSurface }?.surface)
        let nestedCellIdentities = nestedSurface.cells.map { ObjectIdentifier($0.content) }
        XCTAssertFalse(rootCellIdentities.isEmpty)
        XCTAssertFalse(nestedCellIdentities.isEmpty)
        XCTAssertGreaterThan(preparedCellPositions.count, 0)
        let initialPreparationCount = preparedCellPositions.count
        let initialRegistryPreparationCount = registry.layoutPreparationCount

        registry.registerDirectMounted("warm-table-390", layout: first)
        defer { registry.releaseDirectMounted("warm-table-390") }
        var last = first
        for _ in 0..<1_000 {
            last = registry.measure(request: request, widthPoints: 390, scale: 1)
            XCTAssertTrue(last === first)
        }

        XCTAssertEqual(registry.layoutPreparationCount, initialRegistryPreparationCount)
        XCTAssertEqual(preparedCellPositions.count, initialPreparationCount)
        XCTAssertEqual(ObjectIdentifier(first), parentIdentity)
        let reusedRoot = try XCTUnwrap(last.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertTrue(reusedRoot === rootSurface)
        XCTAssertEqual(reusedRoot.cells.map { ObjectIdentifier($0.content) }, rootCellIdentities)
        let reusedSnapshot = ViewerTablePresentation.project(
            layout: last,
            owner: ViewerTablePresentationOwner(),
            viewport: .unknown
        )
        let reusedNested = try XCTUnwrap(reusedSnapshot.cells.first { $0.surface !== rootSurface }?.surface)
        XCTAssertTrue(reusedNested === nestedSurface)
        XCTAssertEqual(reusedNested.cells.map { ObjectIdentifier($0.content) }, nestedCellIdentities)
    }

    func testUnrelatedProseRevisionKeepsRichTableCellsPreparedWhileRefreshingAnchors() throws {
        let missingInlineFamily = "reuse-inline-font-\(UUID().uuidString)"
        let configuration = ProseViewerConfiguration(
            configJSON: try interactionTableConfig(),
            themeJSON: #"{"viewerAtoms":{"generation":"cell-reuse","revision":"1","nodeTypes":["card"],"estimatedHeights":{"card":36}}}"#,
            imagesEnabled: true
        )
        let initialRequest = ProseViewerRequest(
            source: .json(try cellReuseSource(beforeText: "before table", inlineFontFamily: missingInlineFamily)),
            configuration: configuration
        )
        let replacementRequest = ProseViewerRequest(
            source: .json(try cellReuseSource(beforeText: String(repeating: "updated pre-table prose ", count: 80), inlineFontFamily: missingInlineFamily)),
            configuration: configuration
        )
        let engine = CoreTextProseLayoutEngine()
        var preparedCellPositions: [Int] = []
        var shapeBuilds: [Int] = []
        var boundCellPositions: [Int] = []
        engine.tableCellPreparationObserver = { preparedCellPositions.append($0) }
        engine.tableCellShapeBuildObserver = { shapeBuilds.append($0) }
        engine.tableCellBindingObserver = { boundCellPositions.append($0) }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )

        let initial = registry.measure(request: initialRequest, widthPoints: 390, scale: 1)
        XCTAssertNil(initial.error)
        XCTAssertTrue(
            ViewerFontEnvironment.shared.hasMissingFamilyWarning(
                missingInlineFamily,
                semanticGeneration: initial.key.semanticGenerationIdentity
            )
        )
        registry.registerDirectMounted("cell-reuse-initial", layout: initial)
        defer { registry.releaseDirectMounted("cell-reuse-initial") }
        let initialPreparations = preparedCellPositions.count
        let initialBindings = boundCellPositions.count
        XCTAssertGreaterThan(initialPreparations, 0)
        XCTAssertEqual(shapeBuilds.count, initialPreparations)
        XCTAssertEqual(initialBindings, initialPreparations)
        let initialSnapshot = ViewerTablePresentation.project(
            layout: initial,
            owner: ViewerTablePresentationOwner(),
            viewport: .unknown
        )
        let initialSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let initialShapes = initialSurface.cells.map(\.content.cellShape)
        XCTAssertTrue(initialShapes.allSatisfy { $0 != nil })
        let initialAtom = try XCTUnwrap(initialSnapshot.atoms.first { $0.atom.nodeType == "card" })
        let initialImage = try XCTUnwrap(initial.imageAttachments.first { $0.source == "https://example.test/reuse.png" })
        let initialLink = try XCTUnwrap(initialSnapshot.interactions.first {
            $0.interaction.href == "https://cell.example/link"
        })

        let authoredReplacement = try registry.compileDocument(request: replacementRequest)
        let authoredTable = try XCTUnwrap(authoredReplacement.blocks.first { $0.table != nil }?.table)
        var expectedCellPositions: [Int] = []
        var authoredCardPosition: UInt32?
        var authoredImagePosition: UInt32?
        func collectCurrentRecords(_ table: FfiViewerTable) throws {
            for cell in table.cells {
                expectedCellPositions.append(Int(cell.sourcePos))
                for element in cell.elements {
                    switch element {
                    case let .blockAtom(nodeType, docPos, _, _):
                        if nodeType == "card" { authoredCardPosition = docPos }
                        if nodeType == "image" { authoredImagePosition = docPos }
                    case let .table(tableID):
                        try collectCurrentRecords(try XCTUnwrap(authoredReplacement.tableRecords[tableID]))
                    default:
                        continue
                    }
                }
            }
        }
        try collectCurrentRecords(authoredTable)
        let expectedCardPosition = try XCTUnwrap(authoredCardPosition)
        let expectedImagePosition = try XCTUnwrap(authoredImagePosition)
        let expectedImageID = "\(expectedImagePosition):https://example.test/reuse.png"

        XCTAssertNotEqual(authoredReplacement.semanticKey, initial.key.semanticKey)
        XCTAssertNotEqual(expectedCardPosition, initialAtom.atom.docPos)
        XCTAssertNotEqual(expectedImageID, initialImage.id)
        XCTAssertNotEqual(expectedCellPositions, initialSnapshot.cells.map(\.sourcePosition))

        let replacement = registry.measure(request: replacementRequest, widthPoints: 390, scale: 1)
        XCTAssertNil(replacement.error)
        XCTAssertFalse(replacement === initial)
        XCTAssertTrue(
            ViewerFontEnvironment.shared.hasMissingFamilyWarning(
                missingInlineFamily,
                semanticGeneration: replacement.key.semanticGenerationIdentity
            ),
            "a bound cell must replay its inline missing-family warning for a new semantic generation"
        )
        let replacementSnapshot = ViewerTablePresentation.project(
            layout: replacement,
            owner: ViewerTablePresentationOwner(),
            viewport: .unknown
        )
        let replacementSurface = try XCTUnwrap(replacement.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(replacementSurface.cells.count, initialShapes.count)
        for (initialShape, replacementCell) in zip(initialShapes, replacementSurface.cells) {
            XCTAssertTrue(initialShape === replacementCell.content.cellShape)
        }
        let replacementAtom = try XCTUnwrap(replacementSnapshot.atoms.first { $0.atom.nodeType == "card" })
        let replacementImage = try XCTUnwrap(replacement.imageAttachments.first { $0.source == "https://example.test/reuse.png" })
        let replacementLink = try XCTUnwrap(replacementSnapshot.interactions.first {
            $0.interaction.href == "https://cell.example/link"
        })

        XCTAssertEqual(replacementAtom.atom.docPos, expectedCardPosition)
        XCTAssertEqual(replacementImage.id, expectedImageID)
        XCTAssertEqual(replacementImage.ordinal, 0)
        XCTAssertEqual(replacementSnapshot.cells.map(\.sourcePosition), expectedCellPositions)
        XCTAssertEqual(replacementLink.interaction.href, "https://cell.example/link")
        XCTAssertTrue(replacementLink.sourceIdentity.hasPrefix("\(authoredReplacement.semanticKey):"))
        XCTAssertNotEqual(replacementLink.sourceIdentity, initialLink.sourceIdentity)
        XCTAssertTrue(replacementSnapshot.accessibilityNodes.allSatisfy {
            $0.sourceIdentity.hasPrefix("\(authoredReplacement.semanticKey):")
        })
        let initialLinkAccessibility = try XCTUnwrap(initialSnapshot.accessibilityNodes.first {
            $0.node.role == .link && $0.node.label == "linked cell"
        })
        let replacementLinkAccessibility = try XCTUnwrap(replacementSnapshot.accessibilityNodes.first {
            $0.node.role == .link && $0.node.label == "linked cell"
        })
        XCTAssertNotEqual(initialLinkAccessibility.sourceIdentity, replacementLinkAccessibility.sourceIdentity)

        XCTAssertEqual(initialPreparations, preparedCellPositions.count, "unrelated prose must not prepare unchanged table cells")
        XCTAssertEqual(boundCellPositions.count, initialBindings * 2, "replacement cells must receive fresh bindings")
    }

    func testRealTableArtifactsEvictReleasedOwnersAtProductionBudget() throws {
        let byteBudget = 32 * 1024 * 1024
        let compiledByteBudget = 8 * 1024 * 1024
        let engine = CoreTextProseLayoutEngine()
        var preparedCells = 0
        engine.tableCellPreparationObserver = { _ in preparedCells += 1 }
        let registry = PreparedProseLayoutRegistry(
            byteBudget: byteBudget,
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        var owners: [(name: String, request: ProseViewerRequest, layout: PreparedProseLayout)] = []
        for seed in 0..<256 where registry.layoutRetainedBytesForTesting <= byteBudget {
            let request = ProseViewerRequest(
                source: .json(try tableHeavySource(payloadWordCount: 2_048)),
                configuration: ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true),
                nativeFontRevision: UInt64(seed + 1)
            )
            let layout = registry.measure(request: request, widthPoints: 390, scale: 1)
            XCTAssertNil(layout.error)
            let outer = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
            let snapshot = ViewerTablePresentation.project(
                layout: layout,
                owner: ViewerTablePresentationOwner(),
                viewport: .unknown
            )
            XCTAssertEqual(snapshot.cells.count, 4)
            XCTAssertTrue(snapshot.cells.contains { $0.surface !== outer })
            let name = "table-heavy-\(seed)"
            registry.registerDirectMounted(name, layout: layout)
            owners.append((name, request, layout))
        }
        XCTAssertGreaterThan(registry.layoutRetainedBytesForTesting, byteBudget)
        XCTAssertGreaterThan(preparedCells, 0)
        XCTAssertGreaterThan(registry.cellShapeCatalogRetainedBytesForTesting, 0)
        XCTAssertLessThanOrEqual(registry.compiledDocumentBytesForTesting, compiledByteBudget)

        let first = try XCTUnwrap(owners.first)
        for owner in owners {
            registry.releaseDirectMounted(owner.name)
        }
        XCTAssertLessThanOrEqual(registry.layoutRetainedBytesForTesting, byteBudget)

        let preparationsBeforeRebuild = preparedCells
        let rebuilt = registry.measure(request: first.request, widthPoints: 390, scale: 1)
        XCTAssertFalse(rebuilt === first.layout)
        XCTAssertGreaterThan(preparedCells, preparationsBeforeRebuild)
        XCTAssertLessThanOrEqual(registry.layoutRetainedBytesForTesting, byteBudget)
    }

    func testShiftedUndeclaredTableImageReusesCurrentOwnerIntrinsicGeometry() throws {
        ViewerImageIntrinsicStore.shared.clearAndSetEntryLimitForTesting(8)
        defer { ViewerImageIntrinsicStore.shared.clearAndSetEntryLimitForTesting() }
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds = 0
        engine.tableCellShapeBuildObserver = { _ in shapeBuilds += 1 }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let configuration = ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true)
        let initialRequest = ProseViewerRequest(
            source: .json(try deferredImageTableSource()),
            configuration: configuration
        )
        let provisional = registry.measure(request: initialRequest, widthPoints: 390, scale: 1)
        let initialImage = try XCTUnwrap(provisional.imageAttachments.first)
        XCTAssertNil(initialImage.declaredSize)

        let state = ViewerAttachmentRevisionState()
        _ = state.beginSemanticGeneration("shifted-image-owner")
        state.admit(attachmentCount: 1)
        XCTAssertTrue(state.recordIntrinsicSize(CGSize(width: 40, height: 96), for: initialImage.id, ordinal: 0, declaredSize: nil))

        let intrinsicRequest = ProseViewerRequest(
            source: initialRequest.source,
            configuration: configuration,
            attachmentRevision: 1
        )
        let intrinsic = registry.measure(
            request: intrinsicRequest,
            widthPoints: 390,
            scale: 1,
            measurementImageState: state
        )
        let intrinsicImage = try XCTUnwrap(intrinsic.imageAttachments.first)
        let intrinsicMetrics = try tableImageMetrics(layout: intrinsic, attachmentID: intrinsicImage.id)
        registry.registerDirectMounted("shifted-image-owner", layout: intrinsic)
        defer { registry.releaseDirectMounted("shifted-image-owner") }
        let buildsAfterIntrinsic = shapeBuilds

        let shiftedRequest = ProseViewerRequest(
            source: .json(try deferredImageTableSource(beforeText: String(repeating: "moved anchor ", count: 80))),
            configuration: configuration,
            attachmentRevision: 1
        )
        let incomingState = ViewerAttachmentRevisionState()
        _ = incomingState.beginSemanticGeneration("shifted-image-new-semantic-owner")
        incomingState.admit(attachmentCount: 1)
        let shifted = registry.measure(
            request: shiftedRequest,
            widthPoints: 390,
            scale: 1,
            measurementImageState: incomingState
        )
        let shiftedImage = try XCTUnwrap(shifted.imageAttachments.first)
        let shiftedMetrics = try tableImageMetrics(layout: shifted, attachmentID: shiftedImage.id)
        XCTAssertNotEqual(shiftedImage.id, intrinsicImage.id)
        XCTAssertEqual(shapeBuilds, buildsAfterIntrinsic, "a source shift must use the current owner's bounded source fallback")
        XCTAssertEqual(shiftedMetrics.imageBounds.height, intrinsicMetrics.imageBounds.height, accuracy: 0.01)
        XCTAssertEqual(shiftedMetrics.cellHeight, intrinsicMetrics.cellHeight, accuracy: 0.01)
        XCTAssertEqual(shiftedMetrics.tableHeight, intrinsicMetrics.tableHeight, accuracy: 0.01)

        let coldRegistry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try CoreTextProseLayoutEngine().prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            }
        )
        let fresh = coldRegistry.measure(
            request: shiftedRequest,
            widthPoints: 390,
            scale: 1,
            measurementImageState: incomingState
        )
        let freshMetrics = try tableImageMetrics(layout: fresh, attachmentID: shiftedImage.id)
        XCTAssertEqual(shiftedMetrics.imageBounds, freshMetrics.imageBounds)
        XCTAssertEqual(shiftedMetrics.cellHeight, freshMetrics.cellHeight, accuracy: 0.01)
        XCTAssertEqual(shiftedMetrics.tableHeight, freshMetrics.tableHeight, accuracy: 0.01)
    }

    func testImageIntrinsicResolverPrioritizesCurrentOwnerThenEvictsGlobalSourceFallback() throws {
        let store = ViewerImageIntrinsicStore.shared
        store.clearAndSetEntryLimitForTesting(1)
        defer { store.clearAndSetEntryLimitForTesting() }
        let source = "https://example.test/shared-resource.png"
        let priorID = "7:\(source)"
        let currentID = "99:\(source)"
        store.store(CGSize(width: 80, height: 40), for: priorID)

        let state = ViewerAttachmentRevisionState()
        _ = state.beginSemanticGeneration("current-owner")
        state.admit(attachmentCount: 1)
        XCTAssertTrue(state.recordIntrinsicSize(CGSize(width: 30, height: 60), for: priorID, ordinal: 0, declaredSize: nil))
        store.store(CGSize(width: 80, height: 40), for: "8:\(source)")
        let scoped = FabricAttachmentSidecars.withMeasurementState(state) {
            store.size(for: currentID, source: source)
        }
        XCTAssertEqual(scoped, CGSize(width: 30, height: 60))

        let global = store.size(for: currentID, source: source)
        XCTAssertEqual(global, CGSize(width: 80, height: 40))
        store.store(CGSize(width: 1, height: 1), for: "1:https://example.test/evict.png")
        XCTAssertNil(store.size(for: currentID, source: source))
    }

    func testNestedAtomMeasurementsInvalidateOnlyAffectedShapesAndSurviveAnchorShift() throws {
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds: [Int] = []
        engine.tableCellShapeBuildObserver = { shapeBuilds.append($0) }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let probeRequest = ProseViewerRequest(
            source: .json(try nestedAtomReuseSource(beforeText: "before")),
            configuration: ProseViewerConfiguration(
                configJSON: Self.config,
                themeJSON: viewerAtomTheme(position: 0, width: 0, height: 24, generation: "probe"),
                collapsesWhenEmpty: true
            )
        )
        let probe = registry.measure(request: probeRequest, widthPoints: 390, scale: 1)
        let probeAtom = try XCTUnwrap(ViewerTablePresentation.project(layout: probe, owner: ViewerTablePresentationOwner(), viewport: .unknown).atoms.first)
        let measuredWidth = probeAtom.atom.bounds.width
        let initialDocument = try registry.compileDocument(request: probeRequest)
        let initialPosition = try firstCardPosition(in: initialDocument)
        shapeBuilds.removeAll()

        func request(beforeText: String, position: UInt32, height: CGFloat, generation: String) throws -> ProseViewerRequest {
            ProseViewerRequest(
                source: .json(try nestedAtomReuseSource(beforeText: beforeText)),
                configuration: ProseViewerConfiguration(
                    configJSON: Self.config,
                    themeJSON: viewerAtomTheme(position: position, width: measuredWidth, height: height, generation: generation),
                    collapsesWhenEmpty: true
                )
            )
        }

        let initialRequest = try request(beforeText: "before", position: initialPosition, height: 30, generation: "initial")
        let initial = registry.measure(request: initialRequest, widthPoints: 390, scale: 1)
        let initialSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let initialRowOffset = initialSurface.layout.rowOffsets[1]
        registry.registerDirectMounted("nested-atom-initial", layout: initial)
        defer { registry.releaseDirectMounted("nested-atom-initial") }
        let buildsAfterInitial = shapeBuilds.count
        XCTAssertGreaterThan(buildsAfterInitial, 0)

        let shiftedSource = String(repeating: "unrelated anchor ", count: 60)
        let shiftedProbe = ProseViewerRequest(
            source: .json(try nestedAtomReuseSource(beforeText: shiftedSource)),
            configuration: probeRequest.configuration
        )
        let shiftedPosition = try firstCardPosition(in: registry.compileDocument(request: shiftedProbe))
        let shiftedRequest = try request(beforeText: shiftedSource, position: shiftedPosition, height: 30, generation: "bookkeeping-only")
        let shifted = registry.measure(request: shiftedRequest, widthPoints: 390, scale: 1)
        let shiftedSurface = try XCTUnwrap(shifted.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(shapeBuilds.count, buildsAfterInitial)
        XCTAssertEqual(shiftedSurface.layout.rowOffsets[1], initialRowOffset, accuracy: 0.01)

        let changedRequest = try request(beforeText: "before", position: initialPosition, height: 96, generation: "local-height-change")
        let changed = registry.measure(request: changedRequest, widthPoints: 390, scale: 1)
        let changedSurface = try XCTUnwrap(changed.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(shapeBuilds.count - buildsAfterInitial, 2, "only the nested atom cell and its ancestor should reshape")
        XCTAssertGreaterThan(changedSurface.layout.rowOffsets[1], initialRowOffset)

        let laterShiftSource = String(repeating: "later unrelated anchor ", count: 80)
        let laterShiftProbe = ProseViewerRequest(
            source: .json(try nestedAtomReuseSource(beforeText: laterShiftSource)),
            configuration: probeRequest.configuration
        )
        let laterShiftPosition = try firstCardPosition(in: registry.compileDocument(request: laterShiftProbe))
        let laterShiftRequest = try request(beforeText: laterShiftSource, position: laterShiftPosition, height: 96, generation: "changed-bookkeeping-only")
        let buildsAfterChange = shapeBuilds.count
        let laterShift = registry.measure(request: laterShiftRequest, widthPoints: 390, scale: 1)
        let laterShiftSurface = try XCTUnwrap(laterShift.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(shapeBuilds.count, buildsAfterChange)
        XCTAssertEqual(laterShiftSurface.layout.rowOffsets[1], changedSurface.layout.rowOffsets[1], accuracy: 0.01)
    }

    func testCellShapeCatalogDropsFailedAndReleasedParentOwners() throws {
        let engine = CoreTextProseLayoutEngine()
        var failAfterShapeBuild = true
        let registry = PreparedProseLayoutRegistry(
            byteBudget: 1,
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                let layout = try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
                if failAfterShapeBuild { throw ProseViewerError.layout(message: "intentional owner failure") }
                return layout
            }
        )
        let request = ProseViewerRequest(
            source: .json(try cellReuseSource(beforeText: "owner failure")),
            configuration: ProseViewerConfiguration(configJSON: try interactionTableConfig(), collapsesWhenEmpty: true)
        )
        let failed = registry.measure(request: request, widthPoints: 390, scale: 1)
        XCTAssertNotNil(failed.error)
        XCTAssertEqual(registry.cellShapeCatalogCountForTesting, 0)
        XCTAssertEqual(registry.cellShapeCatalogRetainedBytesForTesting, 0)

        failAfterShapeBuild = false
        let successfulRequest = ProseViewerRequest(
            source: request.source,
            configuration: request.configuration,
            nativeFontRevision: 1
        )
        let successful = registry.measure(request: successfulRequest, widthPoints: 390, scale: 1)
        XCTAssertNil(successful.error)
        XCTAssertEqual(registry.cellShapeCatalogCountForTesting, 0, "an unowned oversized result cannot pin catalog entries")
        registry.registerDirectMounted("cell-shape-owner", layout: successful)
        XCTAssertGreaterThan(registry.cellShapeCatalogCountForTesting, 0)
        XCTAssertGreaterThan(registry.cellShapeCatalogRetainedBytesForTesting, 0)
        registry.releaseDirectMounted("cell-shape-owner")
        XCTAssertEqual(registry.cellShapeCatalogCountForTesting, 0)
        XCTAssertEqual(registry.cellShapeCatalogRetainedBytesForTesting, 0)
        XCTAssertLessThanOrEqual(registry.layoutRetainedBytesForTesting, 1)
    }

    func testFailedReplacementReleasesItsPinsAfterFinalParentRetiresDuringBuild() throws {
        let engine = CoreTextProseLayoutEngine()
        var failReplacement = false
        var retiredDuringReplacement = false
        var pinnedEntriesDuringFailure = 0
        var registry: PreparedProseLayoutRegistry!
        registry = PreparedProseLayoutRegistry(
            byteBudget: 1,
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                let layout = try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
                if failReplacement {
                    registry.releaseDirectMounted("retiring-parent")
                    retiredDuringReplacement = true
                    pinnedEntriesDuringFailure = registry.cellShapeCatalogCountForTesting
                    throw ProseViewerError.layout(message: "intentional replacement failure")
                }
                return layout
            }
        )
        let request = ProseViewerRequest(
            source: .json(try cellReuseSource(beforeText: "retiring parent")),
            configuration: ProseViewerConfiguration(configJSON: try interactionTableConfig(), collapsesWhenEmpty: true)
        )
        let initial = registry.measure(request: request, widthPoints: 390, scale: 1)
        registry.registerDirectMounted("retiring-parent", layout: initial)
        XCTAssertGreaterThan(registry.cellShapeCatalogCountForTesting, 0)

        failReplacement = true
        let replacement = registry.measure(
            request: ProseViewerRequest(
                source: request.source,
                configuration: request.configuration,
                nativeFontRevision: 1
            ),
            widthPoints: 390,
            scale: 1
        )
        XCTAssertNotNil(replacement.error)
        XCTAssertTrue(retiredDuringReplacement)
        XCTAssertGreaterThan(pinnedEntriesDuringFailure, 0)
        XCTAssertEqual(registry.cellShapeCatalogCountForTesting, 0)
        XCTAssertEqual(registry.cellShapeCatalogRetainedBytesForTesting, 0)
    }

    func testCellShapeCatalogKeepsSharedBuildPinUntilEveryContextCloses() throws {
        let catalog = PreparedCellShapeCatalog()
        let key = PreparedCellShapeKey(
            contentKey: "shared-build",
            widthPixels: 320,
            scaleBits: Double(1).bitPattern,
            styleDigest: "style",
            atomGeometryDigest: "",
            imageGeometryDigest: ""
        )
        let layout = PreparedProseLayout(
            key: ProseLayoutKey(
                semanticKey: "shared-build",
                widthPixels: 320,
                themeDigest: "theme",
                nativeFontRevision: 0,
                fontEnvironmentRevision: 0,
                displayScale: 1,
                attachmentRevision: 0,
                generationIdentity: "shared-build",
                semanticGenerationIdentity: "shared-build"
            ),
            size: CGSize(width: 320, height: 10),
            blocks: [],
            retainedBytes: 128
        )
        let first = catalog.newBuildContext()
        _ = try first.resolve(key, build: { layout }, bind: { _ in nil })
        let second = catalog.newBuildContext()
        _ = try second.resolve(key, build: {
            XCTFail("the indexed shape should be acquired by the second context")
            return layout
        }, bind: { $0.localLayout })
        XCTAssertEqual(catalog.countForTesting, 1)
        first.close()
        XCTAssertEqual(catalog.countForTesting, 1)
        second.close()
        XCTAssertEqual(catalog.countForTesting, 0)
    }

    func testEqualContentCellsShareOneShapeButBindDistinctShiftedAnchors() throws {
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds = 0
        engine.tableCellShapeBuildObserver = { _ in shapeBuilds += 1 }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let configuration = ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true)
        let initialRequest = ProseViewerRequest(source: .json(try identicalCellSource(beforeText: "before")), configuration: configuration)
        let initial = registry.measure(request: initialRequest, widthPoints: 390, scale: 1)
        let initialSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertTrue(initialSurface.cells[0].content.cellShape === initialSurface.cells[1].content.cellShape)
        XCTAssertEqual(shapeBuilds, 1)
        registry.registerDirectMounted("identical-cell-owner", layout: initial)
        defer { registry.releaseDirectMounted("identical-cell-owner") }

        let replacementRequest = ProseViewerRequest(
            source: .json(try identicalCellSource(beforeText: String(repeating: "shifted prose ", count: 60))),
            configuration: configuration
        )
        let replacement = registry.measure(request: replacementRequest, widthPoints: 390, scale: 1)
        let replacementSurface = try XCTUnwrap(replacement.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertTrue(replacementSurface.cells[0].content.cellShape === replacementSurface.cells[1].content.cellShape)
        XCTAssertEqual(shapeBuilds, 1)
        XCTAssertNotEqual(initialSurface.cells.map(\.sourcePosition), replacementSurface.cells.map(\.sourcePosition))
        XCTAssertNotEqual(replacementSurface.cells[0].sourcePosition, replacementSurface.cells[1].sourcePosition)
    }

    func testChangedCellTextReshapesOnlyItsCellAndPreservesSiblingShape() throws {
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds = 0
        engine.tableCellShapeBuildObserver = { _ in shapeBuilds += 1 }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let configuration = ProseViewerConfiguration(configJSON: try interactionTableConfig(), collapsesWhenEmpty: true)
        let initial = registry.measure(
            request: ProseViewerRequest(source: .json(try selectiveTextCellSource(firstText: "original cell")), configuration: configuration),
            widthPoints: 390,
            scale: 1
        )
        let initialSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let initialBuilds = shapeBuilds
        registry.registerDirectMounted("selective-text-owner", layout: initial)
        defer { registry.releaseDirectMounted("selective-text-owner") }

        let replacementText = "replacement cell text that needs a distinct shape"
        let replacement = registry.measure(
            request: ProseViewerRequest(source: .json(try selectiveTextCellSource(firstText: replacementText)), configuration: configuration),
            widthPoints: 390,
            scale: 1
        )
        let replacementSurface = try XCTUnwrap(replacement.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(shapeBuilds - initialBuilds, 1)
        XCTAssertFalse(initialSurface.cells[0].content.cellShape === replacementSurface.cells[0].content.cellShape)
        XCTAssertTrue(initialSurface.cells[1].content.cellShape === replacementSurface.cells[1].content.cellShape)
        XCTAssertEqual(replacementSurface.cells[0].content.interactions.map(\.visibleText), [replacementText])
        XCTAssertEqual(replacementSurface.cells[0].content.interactions.map(\.href), ["https://example.com/cell"])
        let replacementLines = replacementSurface.cells[0].content.blocks
            .flatMap(\.fragments)
            .filter { $0.kind == .text }
            .compactMap(\.line)
        XCTAssertEqual(replacementLines.reduce(0) { $0 + CTLineGetStringRange($1).length }, replacementText.utf16.count)
        let replacementGlyphCount = replacementLines.reduce(0) { total, line in
            total + ((CTLineGetGlyphRuns(line) as? [CTRun])?.reduce(0) { $0 + CTRunGetGlyphCount($1) } ?? 0)
        }
        XCTAssertGreaterThan(replacementGlyphCount, 0)
    }

    func testDefaultRegistryReusesCellShapesAcrossSemanticRevision() throws {
        let registry = PreparedProseLayoutRegistry()
        let configuration = ProseViewerConfiguration(
            configJSON: try interactionTableConfig(),
            themeJSON: #"{"viewerAtoms":{"nodeTypes":["card"],"estimatedHeights":{"card":36}}}"#,
            imagesEnabled: true
        )
        let initial = registry.measure(
            request: ProseViewerRequest(source: .json(try cellReuseSource(beforeText: "default registry")), configuration: configuration),
            widthPoints: 390,
            scale: 1
        )
        let initialSurface = try XCTUnwrap(initial.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let initialShapes = initialSurface.cells.map(\.content.cellShape)
        XCTAssertTrue(initialShapes.allSatisfy { $0 != nil })
        registry.registerDirectMounted("default-registry-owner", layout: initial)
        defer { registry.releaseDirectMounted("default-registry-owner") }
        let replacement = registry.measure(
            request: ProseViewerRequest(
                source: .json(try cellReuseSource(beforeText: String(repeating: "default shifted prose ", count: 60))),
                configuration: configuration
            ),
            widthPoints: 390,
            scale: 1
        )
        let replacementSurface = try XCTUnwrap(replacement.blocks.first { $0.tableSurface != nil }?.tableSurface)
        for (old, current) in zip(initialShapes, replacementSurface.cells.map(\.content.cellShape)) {
            XCTAssertTrue(old === current)
        }
    }

    func testCellShapeKeyInvalidatesPhysicalWidthStyleFontEnvironmentAndScale() throws {
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds = 0
        engine.tableCellShapeBuildObserver = { _ in shapeBuilds += 1 }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let source = try identicalCellSource(beforeText: "key matrix")
        func request(theme: String, nativeFontRevision: UInt64 = 0, environmentRevision: UInt64 = 0) -> ProseViewerRequest {
            ProseViewerRequest(
                source: .json(source),
                configuration: ProseViewerConfiguration(configJSON: Self.config, themeJSON: theme, collapsesWhenEmpty: true),
                nativeFontRevision: nativeFontRevision,
                fontEnvironmentRevision: environmentRevision
            )
        }
        let base = request(theme: #"{"text":{"fontSize":16}}"#)
        _ = registry.measure(request: base, widthPoints: 390, scale: 1)
        let afterBase = shapeBuilds
        _ = registry.measure(request: base, widthPoints: 400, scale: 1)
        let afterWidth = shapeBuilds
        _ = registry.measure(request: request(theme: #"{"text":{"fontSize":22},"list":{"markerGap":11}}"#), widthPoints: 390, scale: 1)
        let afterStyle = shapeBuilds
        _ = registry.measure(request: request(theme: #"{"text":{"fontSize":16}}"#, nativeFontRevision: 1), widthPoints: 390, scale: 1)
        let afterNativeFont = shapeBuilds
        _ = registry.measure(request: request(theme: #"{"text":{"fontSize":16}}"#, environmentRevision: 1), widthPoints: 390, scale: 1)
        let afterEnvironment = shapeBuilds
        _ = registry.measure(request: base, widthPoints: 195, scale: 2)

        XCTAssertGreaterThan(afterWidth, afterBase)
        XCTAssertGreaterThan(afterStyle, afterWidth)
        XCTAssertGreaterThan(afterNativeFont, afterStyle)
        XCTAssertGreaterThan(afterEnvironment, afterNativeFont)
        XCTAssertGreaterThan(shapeBuilds, afterEnvironment)
    }

    func testOrderedMarkerStyleInvalidatesCellShapeAndPreparedMarker() throws {
        let engine = CoreTextProseLayoutEngine()
        var shapeBuilds = 0
        engine.tableCellShapeBuildObserver = { _ in shapeBuilds += 1 }
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let source = try orderedMarkerCellSource()
        let config = try orderedListTableConfig()
        func request(_ marker: String) -> ProseViewerRequest {
            ProseViewerRequest(
                source: .json(source),
                configuration: ProseViewerConfiguration(
                    configJSON: config,
                    themeJSON: marker,
                    collapsesWhenEmpty: true
                )
            )
        }
        let decimal = registry.measure(
            request: request(#"{"list":{"orderedMarker":{"schemes":["decimal"],"suffix":"."}}}"#),
            widthPoints: 390,
            scale: 1
        )
        let decimalLabel = try XCTUnwrap(decimal.blocks.first { $0.tableSurface != nil }?.tableSurface?.cells.first?.content.blocks.flatMap(\.fragments).first { $0.kind == .marker }?.label)
        let buildsAfterDecimal = shapeBuilds
        let roman = registry.measure(
            request: request(#"{"list":{"orderedMarker":{"schemes":["upperRoman"],"suffix":")"}}}"#),
            widthPoints: 390,
            scale: 1
        )
        let romanLabel = try XCTUnwrap(roman.blocks.first { $0.tableSurface != nil }?.tableSurface?.cells.first?.content.blocks.flatMap(\.fragments).first { $0.kind == .marker }?.label)
        XCTAssertEqual(decimalLabel, "1.")
        XCTAssertEqual(romanLabel, "I)")
        XCTAssertGreaterThan(shapeBuilds, buildsAfterDecimal)
    }

    func testDeferredTableImageRejectsExpiredOwnerAndRemeasuresCurrentOwner() throws {
        let transport = IndividuallyHoldingImageTransport()
        let image = UIGraphicsImageRenderer(size: CGSize(width: 40, height: 96)).image { _ in }
        let decoder = RecordingImageDecoder(image: image)
        let nativeOwner = NativeImagePipeline(
            policy: .default,
            transport: transport,
            decoder: decoder
        )
        let drawing = PreparedProseDrawingView(
            frame: CGRect(x: 0, y: 0, width: 390, height: 600),
            imagePipeline: ViewerImagePipeline(policy: .default, owner: nativeOwner)
        )
        let engine = CoreTextProseLayoutEngine()
        let registry = PreparedProseLayoutRegistry(
            compile: PreparedProseLayoutRegistry.compileWithRust,
            prepare: { document, key, width, scale in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale)
            },
            prepareWithCellShapeContext: { document, key, width, scale, context in
                try engine.prepare(document: document, key: key, widthPoints: width, displayScale: scale, cellShapeContext: context)
            }
        )
        let request = ProseViewerRequest(
            source: .json(try deferredImageTableSource()),
            configuration: ProseViewerConfiguration(configJSON: Self.config, collapsesWhenEmpty: true)
        )
        let oldLayout = registry.measure(request: request, widthPoints: 390, scale: 1)
        XCTAssertNil(oldLayout.error)
        let oldSurface = try XCTUnwrap(oldLayout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let authored = try XCTUnwrap(oldLayout.imageAttachments.first)
        let oldMetrics = try tableImageMetrics(layout: oldLayout, attachmentID: authored.id)
        XCTAssertNil(authored.declaredSize)
        XCTAssertEqual(authored.source, "https://example.test/task16-deferred-table-image.png")
        XCTAssertGreaterThanOrEqual(authored.ordinal, 0)
        XCTAssertEqual(oldMetrics.targetRow, 1)
        registry.registerDirectMounted("old-image-owner", layout: oldLayout)
        drawing.install(layout: oldLayout)
        let window = UIWindow(frame: drawing.frame)
        window.addSubview(drawing)
        window.isHidden = false
        defer {
            drawing.cancelConfiguredImages()
            registry.releaseDirectMounted("new-image-owner")
            window.isHidden = true
        }

        drawing.configureImages(
            generation: request.semanticGenerationIdentity,
            imagesEnabled: true,
            policyJSON: nil
        )
        drawing.updateConfiguredImagesForVisibleWindow()
        XCTAssertTrue(waitForTransportRequests(transport, count: 1))

        registry.releaseDirectMounted("old-image-owner")
        drawing.cancelConfiguredImages()
        registry.registerDirectMounted("new-image-owner", layout: oldLayout)
        drawing.install(layout: oldLayout)
        drawing.configureImages(
            generation: request.semanticGenerationIdentity,
            imagesEnabled: true,
            policyJSON: nil
        )
        drawing.updateConfiguredImagesForVisibleWindow()
        XCTAssertTrue(waitForTransportRequests(transport, count: 2))
        let replacementLayout = try XCTUnwrap(drawing.layout)
        let replacementMetrics = try tableImageMetrics(layout: replacementLayout, attachmentID: authored.id)
        let replacementRevision = drawing.imageRevisionForTesting

        transport.complete(request: 0, with: .success(Data([1])))
        // A synchronous owner read drains its queued transport callback.
        _ = nativeOwner.policy
        flushMain(until: { transport.completedRequestCount >= 1 })
        XCTAssertEqual(drawing.imageRevisionForTesting, replacementRevision)
        XCTAssertNil(drawing.imageRevisionStateForTesting.intrinsicSize(for: authored.ordinal))
        XCTAssertTrue(drawing.layout === replacementLayout)
        let staleMetrics = try tableImageMetrics(layout: replacementLayout, attachmentID: authored.id)
        XCTAssertEqual(staleMetrics.imageBounds, replacementMetrics.imageBounds)
        XCTAssertEqual(staleMetrics.cellHeight, replacementMetrics.cellHeight)
        XCTAssertEqual(staleMetrics.targetRowHeight, replacementMetrics.targetRowHeight)
        XCTAssertEqual(staleMetrics.unrelatedRowHeight, replacementMetrics.unrelatedRowHeight)
        XCTAssertEqual(staleMetrics.tableHeight, replacementMetrics.tableHeight)
        XCTAssertEqual(staleMetrics.fullHeight, replacementMetrics.fullHeight)
        XCTAssertEqual(try XCTUnwrap(replacementLayout.imageAttachments.first).ordinal, authored.ordinal)

        transport.complete(request: 1, with: .success(Data([2])))
        _ = nativeOwner.policy
        flushMain(until: { drawing.imagePixels[authored.id] != nil })
        XCTAssertEqual(drawing.imageRevisionForTesting, replacementRevision + 1)
        let intrinsicSize = image.size.applying(CGAffineTransform(scaleX: image.scale, y: image.scale))
        XCTAssertEqual(drawing.imageRevisionStateForTesting.intrinsicSize(for: authored.ordinal), intrinsicSize)
        XCTAssertEqual(decoder.decodeCount, 1)

        let remeasuredRequest = ProseViewerRequest(
            source: request.source,
            configuration: request.configuration,
            attachmentRevision: drawing.imageRevisionForTesting
        )
        let remeasured = registry.measure(
            request: remeasuredRequest,
            widthPoints: 390,
            scale: 1,
            measurementImageState: drawing.imageRevisionStateForTesting
        )
        let currentMetrics = try tableImageMetrics(layout: remeasured, attachmentID: authored.id)
        let currentSurface = try XCTUnwrap(remeasured.blocks.first { $0.tableSurface != nil }?.tableSurface)
        XCTAssertEqual(try XCTUnwrap(remeasured.imageAttachments.first).id, authored.id)
        XCTAssertTrue(oldSurface.cells[0].content.cellShape === currentSurface.cells[0].content.cellShape)
        XCTAssertFalse(oldSurface.cells[1].content.cellShape === currentSurface.cells[1].content.cellShape)
        XCTAssertNotEqual(currentMetrics.imageBounds.height, oldMetrics.imageBounds.height)
        XCTAssertNotEqual(currentMetrics.cellHeight, oldMetrics.cellHeight)
        XCTAssertNotEqual(currentMetrics.targetRowHeight, oldMetrics.targetRowHeight)
        XCTAssertEqual(currentMetrics.unrelatedRowHeight, oldMetrics.unrelatedRowHeight)
        XCTAssertNotEqual(currentMetrics.tableHeight, oldMetrics.tableHeight)
        XCTAssertNotEqual(currentMetrics.fullHeight, oldMetrics.fullHeight)
    }

    func testMountedRichNestedTablesNeverCreateInputSurfaces() throws {
        let layout = try prepare(try nestedHeaderImageSource(nestedOverflow: true))
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        let window = UIWindow(frame: drawing.bounds)
        window.addSubview(drawing)
        window.isHidden = false
        defer { window.isHidden = true }

        XCTAssertFalse(containsTextInput(in: drawing))
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        _ = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        drawing.setTableLogicalOffset(surface.bounds.width - surface.hostViewportWidth, sourceIdentity: surface.identity)
        XCTAssertFalse(containsTextInput(in: drawing))

        let sensitivityControl = UITextView(frame: .zero)
        drawing.addSubview(sensitivityControl)
        XCTAssertTrue(containsTextInput(in: drawing))
        sensitivityControl.removeFromSuperview()
        XCTAssertFalse(containsTextInput(in: drawing))
    }

    private func prepare(_ source: String, themeJSON: String? = nil, configJSON: String? = nil) throws -> PreparedProseLayout {
        var result = viewerCompile(request: FfiViewerCompileRequest(sourceKind: .json, source: source, configJson: configJSON ?? Self.config, imagesEnabled: true, mentionPrefix: nil))
        if let error = result.error {
            throw ProseViewerError.compiler(domain: error.domain, code: error.code, message: error.message)
        }
        let document = try ViewerDocument(compiled: try XCTUnwrap(result.value))
        result.value = nil
        return try prepare(document, themeJSON: themeJSON)
    }

    private func prepare(
        _ document: ViewerDocument,
        themeJSON: String? = nil,
        engine: CoreTextProseLayoutEngine = CoreTextProseLayoutEngine()
    ) throws -> PreparedProseLayout {
        let theme = PreparedProseTheme.resolve(themeJSON: themeJSON)
        let key = ProseLayoutKey(semanticKey: document.semanticKey, widthPixels: 640, themeDigest: "table", nativeFontRevision: 0, fontEnvironmentRevision: 0, displayScale: 2, attachmentRevision: 0, generationIdentity: "table", semanticGenerationIdentity: "table")
        return try engine.prepare(document: document.withPreparedTheme(theme), key: key, widthPoints: 320, displayScale: 2)
    }

    private func configWithGridSlots(_ slots: Int) throws -> String {
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(Self.config.utf8)) as? [String: Any])
        config["limits"] = ["resource": ["maxTableGridSlots": slots]]
        return try jsonSource(config)
    }

    private func gridLimitSource() -> String {
        #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before ink"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"source survives"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after ink"}]}]}"#
    }

    private func rgba(_ image: CGImage, _ point: CGPoint) throws -> [UInt8] {
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let context = try XCTUnwrap(CGContext(data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let index = Int(point.y) * image.width * 4 + Int(point.x) * 4
        return Array(pixels[index..<(index + 4)])
    }

    private func flushMain(until condition: () -> Bool) {
        let deadline = Date().addingTimeInterval(1)
        repeat {
            let flushed = expectation(description: "flush main queue")
            DispatchQueue.main.async { flushed.fulfill() }
            wait(for: [flushed], timeout: 1)
        } while !condition() && Date() < deadline
    }

    private func prepareHighlighted(_ source: String, generation: String) throws -> PreparedProseLayout {
        var result = viewerCompile(request: FfiViewerCompileRequest(sourceKind: .json, source: source, configJson: Self.config, imagesEnabled: true, mentionPrefix: nil))
        if let error = result.error {
            throw ProseViewerError.compiler(domain: error.domain, code: error.code, message: error.message)
        }
        let document = try ViewerDocument(compiled: try XCTUnwrap(result.value))
        result.value = nil
        var theme = PreparedProseTheme.resolve(themeJSON: nil)
        theme.codeHighlighting = NativeCodeHighlightConfiguration(provider: "table-fixture", theme: "fixture")
        let key = ProseLayoutKey(semanticKey: document.semanticKey, widthPixels: 640, themeDigest: "table-highlight", nativeFontRevision: 0, fontEnvironmentRevision: 0, displayScale: 2, attachmentRevision: 0, generationIdentity: generation, semanticGenerationIdentity: generation)
        return try CoreTextProseLayoutEngine().prepare(document: document.withPreparedTheme(theme), key: key, widthPoints: 320, displayScale: 2)
    }

    private func highlightColor(in layout: PreparedProseLayout) -> CGColor {
        let line = try! XCTUnwrap(layout.blocks.flatMap(\.fragments).first { $0.kind == .text }?.line)
        let run = try! XCTUnwrap((CTLineGetGlyphRuns(line) as? [CTRun])?.first)
        let attributes = CTRunGetAttributes(run) as NSDictionary
        return try! unwrapCoreTextAttribute(attributes[kCTForegroundColorAttributeName], as: CGColor.self)
    }

    private func codeBlock(_ text: String) -> [String: Any] {
        ["type": "codeBlock", "attrs": ["language": "fixture"], "content": [["type": "text", "text": text]]]
    }

    private func paragraph(_ text: String) -> [String: Any] {
        ["type": "paragraph", "content": [["type": "text", "text": text]]]
    }

    private func jsonSource(_ object: [String: Any]) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
        return try XCTUnwrap(String(data: data, encoding: .utf8))
    }

    private func finite(_ rect: CGRect) -> Bool {
        rect.minX.isFinite && rect.minY.isFinite && rect.width.isFinite && rect.height.isFinite
    }

    private func admissionRegistry(counter: PreparationCounter) -> PreparedProseLayoutRegistry {
        PreparedProseLayoutRegistry(
            compile: { request in
                let result = viewerCompile(request: FfiViewerCompileRequest(
                    sourceKind: request.source.kind,
                    source: request.source.value,
                    configJson: request.configuration.configJSON,
                    imagesEnabled: request.configuration.imagesEnabled,
                    mentionPrefix: request.mentionPrefix
                ))
                if let error = result.error {
                    throw ProseViewerError.compiler(domain: error.domain, code: error.code, message: error.message)
                }
                return try ViewerDocument(compiled: try XCTUnwrap(result.value))
            },
            prepare: { _, key, width, _ in
                counter.value += 1
                return PreparedProseLayout(key: key, size: CGSize(width: width, height: 0), blocks: [], retainedBytes: 0)
            }
        )
    }

    private func imageTableSource(imageCount: Int) throws -> String {
        func image(_ index: Int) -> [String: Any] {
            ["type": "image", "attrs": ["src": "https://example.test/\(index).png"]]
        }
        let cellImageCount = imageCount - 1
        let firstCellCount = cellImageCount / 2
        let secondCellCount = cellImageCount - firstCellCount
        return try jsonSource([
            "type": "doc",
            "content": [
                ["type": "table", "content": [
                    ["type": "table_row", "content": [
                        ["type": "table_cell", "content": (0..<firstCellCount).map(image)],
                        ["type": "table_cell", "content": (firstCellCount..<firstCellCount + secondCellCount).map(image)]
                    ]]
                ]],
                image(imageCount - 1)
            ]
        ])
    }

    private func nestedHeaderImageSource(
        imageSource: String = "https://example.test/nested.png",
        nestedOverflow: Bool = false
    ) throws -> String {
        let image: [String: Any] = [
            "type": "image",
            "attrs": ["src": imageSource, "width": 20, "height": 20]
        ]
        let nestedHeader: [String: Any] = ["type": "table_header", "content": [image]]
        let nestedRow: [String: Any] = ["type": "table_row", "content": [nestedHeader] + (nestedOverflow ? [["type": "table_cell", "attrs": ["colwidth": [500]], "content": [paragraph("nested body")]]] : [])]
        let nestedTable: [String: Any] = ["type": "table", "content": [nestedRow]]
        let quote: [String: Any] = ["type": "blockquote", "content": [paragraph("quoted"), nestedTable]]
        let outerHeader: [String: Any] = ["type": "table_header", "attrs": ["colwidth": [300]], "content": [quote]]
        let outerCell: [String: Any] = ["type": "table_cell", "attrs": ["colwidth": [nestedOverflow ? 500 : 300]], "content": [paragraph("body")]]
        let outerRow: [String: Any] = ["type": "table_row", "content": [outerHeader, outerCell]]
        let outerTable: [String: Any] = ["type": "table", "content": [outerRow]]
        return try jsonSource(["type": "doc", "content": [paragraph("before"), outerTable, paragraph("after")]])
    }

    private func cellReuseSource(beforeText: String, inlineFontFamily: String? = nil) throws -> String {
        var linkMarks: [[String: Any]] = [["type": "link", "attrs": ["href": "https://cell.example/link"]]]
        if let inlineFontFamily {
            linkMarks.append(["type": "textStyle", "attrs": ["fontFamily": inlineFontFamily]])
        }
        let link: [String: Any] = [
            "type": "paragraph",
            "content": [[
                "type": "text",
                "text": "linked cell",
                "marks": linkMarks
            ]]
        ]
        let image: [String: Any] = [
            "type": "image",
            "attrs": ["src": "https://example.test/reuse.png", "width": 20, "height": 20]
        ]
        let nested: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [["type": "table_header", "content": [image]]]
        ]]]
        let table: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [
                ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [link, ["type": "card"], nested]],
                ["type": "table_cell", "attrs": ["colwidth": [300]], "content": [paragraph("stable sibling")]]
            ]
        ]]]
        return try jsonSource(["type": "doc", "content": [paragraph(beforeText), table, paragraph("after table")]])
    }

    private func nestedTablesSource(depth: Int) -> String {
        var node: [String: Any] = paragraph("deep")
        for _ in 0..<depth {
            node = ["type": "table", "content": [["type": "table_row", "content": [["type": "table_cell", "content": [node]]]]]]
        }
        return try! jsonSource(["type": "doc", "content": [node]])
    }

    private func tableHeavySource(payloadWordCount: Int) throws -> String {
        let payload = String(repeating: "rich table content ", count: payloadWordCount)
        let richParagraph: [String: Any] = [
            "type": "paragraph",
            "content": [[
                "type": "text",
                "text": payload,
                "marks": [["type": "bold"]]
            ]]
        ]
        let nested: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [
                ["type": "table_header", "content": [richParagraph]],
                ["type": "table_cell", "content": [richParagraph]]
            ]
        ]]]
        let outer: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [
                ["type": "table_header", "content": [richParagraph, nested]],
                ["type": "table_cell", "content": [richParagraph]]
            ]
        ]]]
        return try jsonSource(["type": "doc", "content": [outer]])
    }

    private func deferredImageTableSource(beforeText: String = "before table") throws -> String {
        let image: [String: Any] = [
            "type": "image",
            "attrs": ["src": "https://example.test/task16-deferred-table-image.png"]
        ]
        let table: [String: Any] = ["type": "table", "content": [
            ["type": "table_row", "content": [["type": "table_header", "content": [paragraph("unrelated row")]]]],
            ["type": "table_row", "content": [["type": "table_cell", "content": [paragraph("before image"), image]]]]
        ]]
        return try jsonSource(["type": "doc", "content": [paragraph(beforeText), table, paragraph("after table")]])
    }

    private func nestedAtomReuseSource(beforeText: String) throws -> String {
        let nested: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [["type": "table_cell", "content": [["type": "card"]]]]
        ]]]
        let table: [String: Any] = ["type": "table", "content": [
            ["type": "table_row", "content": [
                ["type": "table_cell", "attrs": ["colwidth": [180]], "content": [nested]],
                ["type": "table_cell", "attrs": ["colwidth": [180]], "content": [paragraph("stable sibling")]]
            ]],
            ["type": "table_row", "content": [
                ["type": "table_cell", "content": [paragraph("later row")]],
                ["type": "table_cell", "content": [paragraph("later sibling")]]
            ]]
        ]]
        return try jsonSource(["type": "doc", "content": [paragraph(beforeText), table]])
    }

    private func identicalCellSource(beforeText: String) throws -> String {
        let content = String(repeating: "same reusable cell content ", count: 40)
        let table: [String: Any] = ["type": "table", "content": [[
            "type": "table_row",
            "content": [
                ["type": "table_cell", "content": [paragraph(content)]],
                ["type": "table_cell", "content": [paragraph(content)]]
            ]
        ]]]
        return try jsonSource(["type": "doc", "content": [paragraph(beforeText), table]])
    }

    private func selectiveTextCellSource(firstText: String) throws -> String {
        let linkedFirstCell: [String: Any] = [
            "type": "paragraph",
            "content": [[
                "type": "text",
                "text": firstText,
                "marks": [["type": "link", "attrs": ["href": "https://example.com/cell"]]]
            ]]
        ]
        let table: [String: Any] = ["type": "table", "content": [[
            "type": "table_row", "content": [
                ["type": "table_cell", "content": [linkedFirstCell]],
                ["type": "table_cell", "content": [paragraph("unchanged sibling")]]
            ]
        ]]]
        return try jsonSource(["type": "doc", "content": [table]])
    }

    private func orderedMarkerCellSource() throws -> String {
        let list: [String: Any] = ["type": "orderedList", "attrs": ["start": 1], "content": [[
            "type": "listItem", "content": [paragraph("marker item")]
        ]]]
        let table: [String: Any] = ["type": "table", "content": [[
            "type": "table_row", "content": [["type": "table_cell", "content": [list]]]
        ]]]
        return try jsonSource(["type": "doc", "content": [table]])
    }

    private func orderedListTableConfig() throws -> String {
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(Self.config.utf8)) as? [String: Any])
        var schema = try XCTUnwrap(config["schema"] as? [String: Any])
        var nodes = try XCTUnwrap(schema["nodes"] as? [[String: Any]])
        nodes.append([
            "name": "orderedList", "content": "listItem+", "group": "block", "role": "list",
            "attrs": ["start": ["type": "number", "default": 1, "min": 1]]
        ])
        schema["nodes"] = nodes
        config["schema"] = schema
        return try jsonSource(config)
    }

    private func viewerAtomTheme(position: UInt32, width: CGFloat, height: CGFloat, generation: String) -> String {
        """
        {"viewerAtoms":{"generation":"\(generation)","revision":"\(generation)","nodeTypes":["card"],"estimatedHeights":{"card":24},"measurements":{"\(position)":{"width":\(width),"height":\(height)}}}}
        """
    }

    private func firstCardPosition(in document: ViewerDocument) throws -> UInt32 {
        for block in document.blocks {
            for inline in block.inlines {
                if case let .atom("card", docPos, _, _) = inline { return docPos }
            }
            if let table = block.table {
                for cell in table.cells {
                    if let position = try? firstCardPosition(in: document.cellDocument(for: cell)) { return position }
                }
            }
        }
        throw ProseViewerError.layout(message: "Expected nested card atom.")
    }

    private func tableImageMetrics(
        layout: PreparedProseLayout,
        attachmentID: String
    ) throws -> (
        imageBounds: CGRect,
        cellHeight: CGFloat,
        targetRow: Int,
        targetRowHeight: CGFloat,
        unrelatedRowHeight: CGFloat,
        tableHeight: CGFloat,
        fullHeight: CGFloat
    ) {
        let image = try XCTUnwrap(layout.imageAttachments.first { $0.id == attachmentID })
        let surface = try XCTUnwrap(layout.blocks.first { $0.tableSurface != nil }?.tableSurface)
        let cell = try XCTUnwrap(surface.cells.first {
            $0.content.imageAttachments.contains { $0.id == attachmentID }
        })
        let sourceIndex = try XCTUnwrap(cell.sourceCellIndex)
        let sourceTable = try XCTUnwrap(surface.sourceTable)
        let sourceCell = try XCTUnwrap(
            sourceTable.cells.indices.contains(sourceIndex) ? sourceTable.cells[sourceIndex] : nil
        )
        let row = Int(sourceCell.row)
        let offsets = surface.layout.rowOffsets
        XCTAssertGreaterThan(offsets.count, row + 1)
        return (
            image.bounds,
            cell.frame.height,
            row,
            offsets[row + 1] - offsets[row],
            offsets[1] - offsets[0],
            surface.bounds.height,
            layout.size.height
        )
    }

    private func waitForTransportRequests(
        _ transport: IndividuallyHoldingImageTransport,
        count: Int
    ) -> Bool {
        let deadline = Date().addingTimeInterval(1)
        repeat {
            if transport.requestCount >= count { return true }
            RunLoop.main.run(until: Date().addingTimeInterval(0.01))
        } while Date() < deadline
        return false
    }

    private func containsTextInput(in view: UIView) -> Bool {
        view is UITextInput || view.subviews.contains { containsTextInput(in: $0) }
    }

    private func configWithMaxDocumentDepth(_ maximum: Int) throws -> String {
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(Self.config.utf8)) as? [String: Any])
        config["limits"] = ["resource": ["maxDocumentDepth": maximum]]
        return try jsonSource(config)
    }

    private func interactionTableConfig() throws -> String {
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(Self.config.utf8)) as? [String: Any])
        var schema = try XCTUnwrap(config["schema"] as? [String: Any])
        var nodes = try XCTUnwrap(schema["nodes"] as? [[String: Any]])
        let textIndex = try XCTUnwrap(nodes.firstIndex { $0["name"] as? String == "text" })
        nodes.insert([
            "name": "mention",
            "content": "",
            "group": "inline",
            "role": "inline",
            "isVoid": true,
            "allowUndeclaredAttrs": true,
            "attrs": ["label": ["default": nil]]
        ], at: textIndex)
        schema["nodes"] = nodes
        schema["marks"] = [
            ["name": "bold"],
            ["name": "link", "attrs": ["href": ["default": ""]]],
            ["name": "textStyle", "attrs": ["fontFamily": ["default": nil], "fontSize": ["default": nil]]]
        ]
        config["schema"] = schema
        return try jsonSource(config)
    }

    private static let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"codeBlock","content":"inline*","group":"block","role":"textBlock","attrs":{"language":{"default":null}}},{"name":"text","content":"","group":"inline","role":"text"},{"name":"blockquote","content":"block+","group":"block","role":"block"},{"name":"bulletList","content":"listItem+","group":"block","role":"list"},{"name":"listItem","content":"block+","role":"listItem"},{"name":"image","content":"","group":"block","role":"block","isVoid":true,"attrs":{"src":{"default":""},"width":{"default":null},"height":{"default":null}}},{"name":"card","content":"","group":"block","role":"block","isVoid":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[{"name":"bold"}]},"initialization":{"type":"localEmpty"}}"#
}

private final class PreparationCounter {
    var value = 0
}

private final class IndividuallyHoldingImageTransport: ImageLoadingTransport {
    private let lock = NSLock()
    private var completions: [(Result<Data, Error>) -> Void] = []
    private var storedRequestCount = 0
    private var storedCompletedRequestCount = 0

    var requestCount: Int { lock.withLock { storedRequestCount } }
    var completedRequestCount: Int { lock.withLock { storedCompletedRequestCount } }

    func load(
        _ url: URL,
        policy: ImageLoadingPolicy,
        completion: @escaping (Result<Data, Error>) -> Void
    ) -> ImageLoadingTask {
        lock.withLock {
            storedRequestCount += 1
            completions.append(completion)
        }
        return DeferredImageLoadingTask()
    }

    func complete(request: Int, with result: Result<Data, Error>) {
        let completion = lock.withLock { () -> ((Result<Data, Error>) -> Void)? in
            guard completions.indices.contains(request) else { return nil }
            storedCompletedRequestCount += 1
            return completions[request]
        }
        completion?(result)
    }
}

private final class DeferredImageLoadingTask: ImageLoadingTask {
    func cancel() {}
}
