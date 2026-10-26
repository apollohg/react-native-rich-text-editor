import UIKit
import XCTest

@MainActor
final class ToolbarIconTests: XCTestCase {
    func testGroupIconChangesBetweenLoadedImageSymbolAndFallback() throws {
        let png = try samplePNG()
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        toolbar.setItemsJSONForTesting("""
        [{"type":"group","key":"format","label":"Format","icon":{"type":"glyph","text":"F"},
          "iconSource":"activeChild","items":[
            {"type":"mark","mark":"bold","label":"Bold","icon":{"type":"image","source":{"uri":"data:image/png;base64,\(png.base64EncodedString())"},"tintable":false}},
            {"type":"mark","mark":"italic","label":"Italic","icon":{"type":"default","id":"italic"}}
          ]}]
        """)
        toolbar.rebuildButtonsIfNeeded()
        let button = try XCTUnwrap(toolbar.buttonBindings.first?.button)
        toolbar.applyStateJSONForTesting("""
        {"activeState":{"marks":{"bold":true},"allowedMarks":["bold","italic"]}}
        """)
        let ready = expectation(for: NSPredicate { _, _ in button.image(for: .normal) != nil }, evaluatedWith: button)
        wait(for: [ready], timeout: 5)
        XCTAssertEqual(button.image(for: .normal)?.renderingMode, .alwaysOriginal)
        toolbar.applyStateJSONForTesting("""
        {"activeState":{"marks":{"italic":true},"allowedMarks":["bold","italic"]}}
        """)
        XCTAssertTrue(button.image(for: .normal)?.isSymbolImage == true)
        toolbar.applyStateJSONForTesting("""
        {"activeState":{"marks":{},"allowedMarks":["bold","italic"]}}
        """)
        XCTAssertNil(button.image(for: .normal), "The previous child's artwork must be cleared")
        XCTAssertEqual(button.title(for: .normal), "F")
        XCTAssertTrue(toolbar.buttonBindings.first?.button === button)
    }

    func testPackagedFontResolvesNonBMPGlyphAndUnknownName() throws {
        let icon = try XCTUnwrap(ToolbarIconFont.icon(named: "format-bold"))
        XCTAssertEqual(icon["fontFamily"], "MaterialDesignIcons")
        XCTAssertEqual(icon["glyph"]?.unicodeScalars.count, 1)
        XCTAssertGreaterThan(try XCTUnwrap(icon["glyph"]?.unicodeScalars.first?.value), UInt32(UInt16.max))
        XCTAssertNotNil(UIFont(name: try XCTUnwrap(icon["fontFamily"]), size: 20))
        XCTAssertEqual(try ToolbarIconFont.icon(named: "format-bold"), icon, "Repeated lookups must not fail font registration")
        XCTAssertNil(try ToolbarIconFont.icon(named: "__unknown_editor_icon__"))
    }

    func testLocalAndDataImagesDecodeAndRespectRenderingModeAndBounds() throws {
        let png = try samplePNG()
        let fileURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).appendingPathExtension("png")
        try png.write(to: fileURL)
        defer { try? FileManager.default.removeItem(at: fileURL) }
        let loader = ToolbarImageLoader()
        let sources = try [fileURL.absoluteString, "data:image/png;base64,\(png.base64EncodedString())"].map {
            try XCTUnwrap(ToolbarImageSource(["uri": $0, "width": 80, "height": 40, "scale": 2]))
        }
        let loaded = expectation(description: "Both local and data images decode")
        loaded.expectedFulfillmentCount = sources.count
        loader.onImageLoaded = { loaded.fulfill() }
        sources.forEach(loader.load)
        wait(for: [loaded], timeout: 5)
        for source in sources {
            let image = try XCTUnwrap(loader.image(for: source))
            XCTAssertLessThanOrEqual(max(image.size.width, image.size.height), CGFloat(ToolbarImageLoader.maximumDecodeDimension))
            for tintable in [true, false] {
                let rendered = ToolbarImageLoader.rendered(image, source: source, size: 24, tintable: tintable)
                XCTAssertEqual(rendered.size, CGSize(width: 24, height: 12))
                XCTAssertEqual(rendered.renderingMode, tintable ? .alwaysTemplate : .alwaysOriginal)
            }
        }
    }

    func testResetDiscardsOldImageDeliveryAndAllowsNewLoad() throws {
        let png = try samplePNG()
        let fileURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).appendingPathExtension("png")
        try png.write(to: fileURL)
        defer { try? FileManager.default.removeItem(at: fileURL) }
        let source = try XCTUnwrap(ToolbarImageSource(["uri": fileURL.absoluteString]))
        let loader = ToolbarImageLoader()
        loader.onImageLoaded = { XCTFail("Cancelled generation must not deliver") }
        loader.load(source)
        loader.reset()
        XCTAssertNil(loader.image(for: source))
        let loaded = expectation(description: "New generation loads the same image")
        loaded.assertForOverFulfill = true
        loader.onImageLoaded = { loaded.fulfill() }
        loader.load(source)
        wait(for: [loaded], timeout: 5)
        XCTAssertNotNil(loader.image(for: source))
    }

    func testImageDescriptorDefaultsToTintAndIgnoresInvalidDimensions() throws {
        let icon = try XCTUnwrap(NativeToolbarIcon.from(jsonValue: [
            "type": "image", "source": ["uri": "asset.png", "width": -1, "height": true, "scale": 0],
            "fallbackText": "Photo"
        ]))
        XCTAssertTrue(icon.tintable)
        XCTAssertNil(icon.imageSource?.width)
        XCTAssertNil(icon.imageSource?.height)
        XCTAssertNil(icon.imageSource?.scale)
        XCTAssertEqual(icon.resolvedGlyphText(), "Photo")
        let original = try XCTUnwrap(NativeToolbarIcon.from(jsonValue: [
            "type": "image", "source": ["uri": "asset.png"], "tintable": false
        ]))
        XCTAssertFalse(original.tintable)
    }

    func testLoadedImageReplacesFallbackWithoutReplacingButtonOrStyle() throws {
        let png = try samplePNG()
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        toolbar.setItemsJSONForTesting("""
        [{"type":"action","key":"photo","label":"Photo","icon":{"type":"image","source":{"uri":"data:image/png;base64,\(png.base64EncodedString())"},"tintable":false,"fallbackText":"Loading"},"buttonStyle":{"iconSize":18,"width":54}}]
        """)
        toolbar.rebuildButtonsIfNeeded()
        let button = try XCTUnwrap(toolbar.buttonBindings.first?.button)
        XCTAssertEqual(button.title(for: .normal), "Loading")
        let ready = expectation(for: NSPredicate { _, _ in button.image(for: .normal) != nil }, evaluatedWith: button)
        wait(for: [ready], timeout: 5)
        XCTAssertTrue(toolbar.buttonBindings.first?.button === button, "An image arrival must not rebuild buttons or dismiss menus")
        XCTAssertNil(button.title(for: .normal))
        XCTAssertEqual(button.image(for: .normal)?.renderingMode, .alwaysOriginal)
        XCTAssertEqual(button.image(for: .normal)?.size.width, 18)
        XCTAssertEqual(toolbar.buttonBindings.first?.widthConstraint.constant, 54)
        toolbar.apply(theme: EditorToolbarTheme(dictionary: ["buttonIconSize": 30]))
        XCTAssertEqual(button.image(for: .normal)?.size.width, 18, "Per-item image sizing takes precedence over the theme")
    }

    func testMenuImageLoadsWithoutReplacingPresentationAndPreservesTintMode() throws {
        let png = try samplePNG()
        let toolbar = EditorAccessoryToolbarView(frame: CGRect(x: 0, y: 0, width: 320, height: 56))
        let window = makeTestWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        window.rootViewController = UIViewController()
        window.makeKeyAndVisible()
        window.rootViewController?.view.addSubview(toolbar)
        defer {
            toolbar.removeFromSuperview()
            window.isHidden = true
        }
        toolbar.setItemsJSONForTesting("""
        [{"type":"group","key":"images","label":"Images","icon":{"type":"glyph","text":"I"},"presentation":"menu","items":[{"type":"action","key":"photo","label":"Photo","icon":{"type":"image","source":{"uri":"data:image/png;base64,\(png.base64EncodedString())"},"tintable":false}},{"type":"action","key":"missing","label":"Missing photo","icon":{"type":"image","source":{"uri":"data:image/png;base64,AA=="},"fallbackText":"!"}},{"type":"action","key":"default-fallback","label":"Unavailable photo","icon":{"type":"image","source":{"uri":"data:image/png;base64,AQ=="}}}]}]
        """)
        let interaction = try XCTUnwrap(toolbar.interactions.compactMap { $0 as? UIEditMenuInteraction }.first)
        defer { interaction.dismissMenu() }
        toolbar.triggerButtonTapForTesting(0)
        let configuration = UIEditMenuConfiguration(identifier: nil, sourcePoint: .zero)
        func menuAction(at index: Int = 0) -> UIAction? {
            interaction.delegate?.editMenuInteraction?(interaction, menuFor: configuration, suggestedActions: [])?.children.dropFirst(index).first as? UIAction
        }
        XCTAssertEqual(menuAction()?.title, "? Photo")
        XCTAssertEqual(menuAction(at: 1)?.title, "! Missing photo")
        XCTAssertEqual(menuAction(at: 2)?.title, "? Unavailable photo")
        let loaded = expectation(for: NSPredicate { _, _ in menuAction()?.image != nil }, evaluatedWith: toolbar)
        wait(for: [loaded], timeout: 5)
        XCTAssertEqual(menuAction()?.title, "Photo", "Successful images remove the pending fallback from their label")
        XCTAssertEqual(menuAction()?.image?.renderingMode, .alwaysOriginal)
        XCTAssertEqual(menuAction()?.state, .off)
        XCTAssertFalse(menuAction()?.attributes.contains(.disabled) ?? true)
        XCTAssertEqual(toolbar.editMenuPresentationRequestCountForTesting, 1)
        XCTAssertEqual(menuAction(at: 1)?.title, "! Missing photo", "Failed images retain fallback text without losing their label")
        XCTAssertEqual(menuAction(at: 1)?.discoverabilityTitle, "Missing photo")
        XCTAssertEqual(menuAction(at: 2)?.title, "? Unavailable photo", "Failed images without fallbackText use the default question mark")
        XCTAssertEqual(menuAction(at: 2)?.discoverabilityTitle, "Unavailable photo")
    }

    private func samplePNG() throws -> Data {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        return try XCTUnwrap(UIGraphicsImageRenderer(size: CGSize(width: 800, height: 400), format: format).image { context in
            UIColor.red.setFill()
            context.fill(CGRect(x: 0, y: 0, width: 800, height: 400))
        }.pngData())
    }
}
