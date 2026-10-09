import ExpoModulesCore
import XCTest

extension RichTextEditorViewTests {
    func testAccessoryToolbarIntrinsicHeightMatchesLayoutFitting() throws {
        let fixtureWidth: CGFloat = 320
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        func assertMatchingHeight(_ state: String, height: CGFloat, line: UInt = #line) {
            let fitted = toolbar.systemLayoutSizeFitting(CGSize(width: fixtureWidth, height: UIView.layoutFittingCompressedSize.height))
            XCTAssertEqual(toolbar.intrinsicContentSize.height, height, accuracy: NativeEditorExpoView.layoutEpsilon,
                "the keyboard and Auto Layout must agree on accessory height in \(state): intrinsic=\(toolbar.intrinsicContentSize), fitted=\(fitted)", line: line)
            XCTAssertEqual(fitted.height, height, accuracy: NativeEditorExpoView.layoutEpsilon, line: line)
        }
        let defaultHeight: CGFloat = 62
        let themedHeight: CGFloat = 86
        let mentionHeight: CGFloat = 74
        assertMatchingHeight("default toolbar", height: defaultHeight)
        toolbar.apply(theme: EditorToolbarTheme(dictionary: ["appearance": "native", "height": 64, "keyboardOffset": 10]))
        assertMatchingHeight("themed toolbar", height: themedHeight)
        let suggestion = try XCTUnwrap(NativeMentionSuggestion(dictionary: ["key": "alice", "title": "Alice", "label": "@alice", "attrs": ["label": "@alice"]]))
        _ = toolbar.setMentionSuggestions([suggestion])
        assertMatchingHeight("mention suggestions", height: mentionHeight)
        _ = toolbar.setMentionSuggestions([])
        assertMatchingHeight("restored toolbar", height: themedHeight)
    }

    func testAccessoryToolbarPreservesContentHeightAboveBottomSafeArea() {
        let window = makeTestWindow(frame: UIScreen.main.bounds)
        let controller = UIViewController()
        window.rootViewController = controller
        let fixtureBottomInset: CGFloat = 20
        controller.additionalSafeAreaInsets.bottom = fixtureBottomInset
        window.makeKeyAndVisible()
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        controller.view.addSubview(toolbar)
        NSLayoutConstraint.activate([
            toolbar.leadingAnchor.constraint(equalTo: controller.view.leadingAnchor),
            toolbar.trailingAnchor.constraint(equalTo: controller.view.trailingAnchor),
            toolbar.bottomAnchor.constraint(equalTo: controller.view.bottomAnchor)
        ])
        defer { window.isHidden = true }
        window.layoutIfNeeded()
        flushMainQueue()
        window.layoutIfNeeded()
        toolbar.layoutIfNeeded()

        XCTAssertGreaterThanOrEqual(toolbar.safeAreaInsets.bottom, fixtureBottomInset, "the fixture must include a bottom safe area")
        XCTAssertEqual(toolbar.bounds.height, toolbar.intrinsicContentSize.height,
            accuracy: NativeEditorExpoView.layoutEpsilon,
            "the accessory must reserve both its content and bottom safe area: bounds=\(toolbar.bounds), intrinsic=\(toolbar.intrinsicContentSize), safe=\(toolbar.safeAreaInsets)")
        let defaultRowHeight: CGFloat = 50
        XCTAssertEqual(toolbar.bodyStackView.bounds.height, defaultRowHeight, accuracy: NativeEditorExpoView.layoutEpsilon)
        XCTAssertTrue(toolbar.safeAreaLayoutGuide.layoutFrame.contains(toolbar.bodyStackView.convert(toolbar.bodyStackView.bounds, to: toolbar)))
    }

    func testKeyboardHostedToolbarResizesForThemeAndMentionChanges() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let window = makeTestWindow(frame: UIScreen.main.bounds)
        let controller = UIViewController()
        window.rootViewController = controller
        window.makeKeyAndVisible()
        let view = NativeEditorExpoView()
        view.frame = window.bounds
        controller.view.addSubview(view)
        view.setEditorId(editorId)
        view.richTextView.setContent(html: "<p>Keyboard accessory sizing</p>")
        view.layoutIfNeeded()
        let input = view.richTextView.textView
        let toolbar = view.accessoryToolbar
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }

        let defaultRowHeight: CGFloat = 50
        let themedRowHeight: CGFloat = 64
        let mentionRowHeight: CGFloat = 52
        let themedKeyboardOffset: CGFloat = 10
        func keyboardHostFrameInScreen() -> CGRect {
            controller.view.convert(controller.view.keyboardLayoutGuide.layoutFrame, to: window.screen.coordinateSpace)
        }
        func assertMountedHeight(_ state: String, rowHeight: CGFloat, keyboardOffset: CGFloat = 0, line: UInt = #line) throws {
            let verticalPadding: CGFloat = 12
            let expectedContentHeight = rowHeight + verticalPadding + keyboardOffset
            let screenBoundsWithTolerance = window.screen.bounds.insetBy(
                dx: -NativeEditorExpoView.layoutEpsilon, dy: -NativeEditorExpoView.layoutEpsilon
            )
            let settled = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
                let keyboardFrame = keyboardHostFrameInScreen()
                let toolbarFrame = toolbar.convert(toolbar.bounds, to: window.screen.coordinateSpace)
                return input.isFirstResponder && toolbar.window != nil && keyboardFrame.height > 0
                    && screenBoundsWithTolerance.contains(toolbarFrame)
                    && toolbarFrame.minY <= keyboardFrame.minY + NativeEditorExpoView.layoutEpsilon
                    && toolbarFrame.maxY + NativeEditorExpoView.layoutEpsilon >= keyboardFrame.minY
                    && abs(toolbar.bounds.height - expectedContentHeight - toolbar.safeAreaInsets.bottom) <= NativeEditorExpoView.layoutEpsilon
                    && abs(toolbar.bodyStackView.bounds.height - rowHeight) <= NativeEditorExpoView.layoutEpsilon
            }, object: toolbar)
            let layoutTimeout: TimeInterval = 3
            wait(for: [settled], timeout: layoutTimeout)
            let expectedHeight = expectedContentHeight + toolbar.safeAreaInsets.bottom
            XCTAssertTrue(input.isFirstResponder, "\(state) must preserve keyboard focus", line: line)
            XCTAssertTrue(input.inputAccessoryView === toolbar, line: line)
            XCTAssertEqual(toolbar.bounds.height, expectedHeight,
                accuracy: NativeEditorExpoView.layoutEpsilon,
                "mounted \(state): bounds=\(toolbar.bounds), intrinsic=\(toolbar.intrinsicContentSize), safe=\(toolbar.safeAreaInsets)", line: line)
            XCTAssertEqual(toolbar.bodyStackView.bounds.height, rowHeight, accuracy: NativeEditorExpoView.layoutEpsilon, line: line)
            XCTAssertTrue(toolbar.safeAreaLayoutGuide.layoutFrame.contains(toolbar.bodyStackView.convert(toolbar.bodyStackView.bounds, to: toolbar)),
                "\(state) must keep the row above the bottom safe area", line: line)
            XCTAssertEqual(toolbar.bounds.width, window.bounds.width, accuracy: NativeEditorExpoView.layoutEpsilon, line: line)
            let keyboardFrame = keyboardHostFrameInScreen()
            let toolbarFrame = toolbar.convert(toolbar.bounds, to: window.screen.coordinateSpace)
            XCTAssertGreaterThan(keyboardFrame.height, 0, "\(state) must have a visible keyboard host", line: line)
            XCTAssertTrue(screenBoundsWithTolerance.contains(toolbarFrame),
                "\(state) must keep the full accessory on screen: toolbar=\(toolbarFrame), screen=\(window.screen.bounds)", line: line)
            XCTAssertLessThanOrEqual(toolbarFrame.minY, keyboardFrame.minY + NativeEditorExpoView.layoutEpsilon,
                "\(state) must start above the keyboard guide: toolbar=\(toolbarFrame), keyboard=\(keyboardFrame)", line: line)
            XCTAssertGreaterThanOrEqual(toolbarFrame.maxY + NativeEditorExpoView.layoutEpsilon, keyboardFrame.minY,
                "\(state) must meet the keyboard guide without a gap: toolbar=\(toolbarFrame), keyboard=\(keyboardFrame)", line: line)
            let occlusionTop = min(toolbarFrame.minY, keyboardFrame.minY)
            let editableViewportBottom = input.convert(
                CGPoint(x: input.bounds.midX, y: input.bounds.maxY - input.adjustedContentInset.bottom),
                to: window.screen.coordinateSpace
            ).y
            XCTAssertLessThanOrEqual(editableViewportBottom, occlusionTop + NativeEditorExpoView.layoutEpsilon,
                "\(state) must keep the editable viewport above the keyboard host: bottom=\(editableViewportBottom), toolbar=\(toolbarFrame), keyboard=\(keyboardFrame), inset=\(input.adjustedContentInset), reported=\(String(describing: input.keyboardFrameInScreen))", line: line)
            let rowFrame = toolbar.bodyStackView.convert(toolbar.bodyStackView.bounds, to: window.screen.coordinateSpace)
            XCTAssertTrue(screenBoundsWithTolerance.contains(rowFrame),
                "\(state) must keep the full toolbar row on screen: row=\(rowFrame)", line: line)
            var ancestor = toolbar.bodyStackView.superview
            while let current = ancestor {
                if current.clipsToBounds {
                    let clippingFrame = current.convert(current.bounds, to: window.screen.coordinateSpace)
                    XCTAssertTrue(clippingFrame.insetBy(
                        dx: -NativeEditorExpoView.layoutEpsilon, dy: -NativeEditorExpoView.layoutEpsilon
                    ).contains(rowFrame),
                        "\(state) row is clipped by \(type(of: current)): row=\(rowFrame), clip=\(clippingFrame)", line: line)
                }
                ancestor = current.superview
            }
            let visibleControl: UIButton
            if toolbar.isShowingMentionSuggestions {
                visibleControl = try XCTUnwrap(toolbar.mentionButtonAtForTesting(0), "\(state) must show a mention chip", line: line)
            } else {
                visibleControl = try XCTUnwrap(toolbar.buttonBindings.first { binding in
                    let button = binding.button
                    let center = button.convert(CGPoint(x: button.bounds.midX, y: button.bounds.midY), to: window.screen.coordinateSpace)
                    return !button.isHidden && button.alpha > 0 && !button.bounds.isEmpty && rowFrame.contains(center)
                }?.button, "\(state) must show a toolbar button", line: line)
            }
            let controlWindow = try XCTUnwrap(visibleControl.window, "\(state) control must belong to the native keyboard window", line: line)
            let controlCenter = visibleControl.convert(
                CGPoint(x: visibleControl.bounds.midX, y: visibleControl.bounds.midY), to: controlWindow
            )
            let hit = controlWindow.hitTest(controlCenter, with: nil)
            XCTAssertTrue(hit === visibleControl || hit?.isDescendant(of: visibleControl) == true,
                "\(state) control must receive native window hit testing: control=\(visibleControl), center=\(controlCenter), hit=\(String(describing: hit))", line: line)
        }

        XCTAssertTrue(input.becomeFirstResponder())
        try assertMountedHeight("default toolbar", rowHeight: defaultRowHeight)
        view.setThemeJson(#"{"toolbar":{"appearance":"native","height":64,"keyboardOffset":10}}"#)
        try assertMountedHeight("themed toolbar", rowHeight: themedRowHeight, keyboardOffset: themedKeyboardOffset)
        let suggestion = try XCTUnwrap(NativeMentionSuggestion(dictionary: [
            "key": "alice", "title": "Alice", "label": "@alice", "attrs": ["label": "@alice"]
        ]))
        _ = toolbar.setMentionSuggestions([suggestion])
        input.reloadInputViews()
        try assertMountedHeight("mention suggestions", rowHeight: mentionRowHeight, keyboardOffset: themedKeyboardOffset)
        let chip = try XCTUnwrap(toolbar.mentionButtonAtForTesting(0))
        XCTAssertGreaterThan(chip.bounds.width, 0)
        XCTAssertGreaterThan(chip.bounds.height, 0)
        XCTAssertTrue(toolbar.bounds.contains(chip.convert(chip.bounds, to: toolbar)))
        _ = toolbar.setMentionSuggestions([])
        input.reloadInputViews()
        try assertMountedHeight("restored themed toolbar", rowHeight: themedRowHeight, keyboardOffset: themedKeyboardOffset)
        view.setThemeJson(nil)
        try assertMountedHeight("restored default toolbar", rowHeight: defaultRowHeight)
        XCTAssertTrue(input.resignFirstResponder())
        XCTAssertTrue(input.becomeFirstResponder())
        try assertMountedHeight("refocused toolbar", rowHeight: defaultRowHeight)
        let typedText = "awake"
        input.insertText(typedText)
        XCTAssertTrue(EditorV2Shadow.getHtml(id: editorId).contains(typedText))
    }

    func testAccessoryToolbarNativeDisabledButtonUsesAdaptiveTintInDarkHost() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        toolbar.tintColor = .black

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        toolbar.applyBoldStateForTesting(active: false, enabled: false)

        XCTAssertEqual(
            toolbar.firstButtonAlphaForTesting, 1.0, accuracy: 0.01,
            "Disabled native button must stay at full alpha because low alpha is invisible on dark blur backgrounds"
        )
        guard let tintColor = toolbar.firstButtonTintColorForTesting else {
            return XCTFail("Disabled native button should apply an explicit transparent tint")
        }
        XCTAssertEqual(tintColor.cgColor.alpha, 0.46, accuracy: 0.01)
        let darkTint = tintColor.resolvedColor(
            with: UITraitCollection(userInterfaceStyle: .dark)
        )
        var white: CGFloat = 0
        var alpha: CGFloat = 0
        XCTAssertTrue(darkTint.getWhite(&white, alpha: &alpha))
        XCTAssertGreaterThan(
            white, 0.9,
            "Disabled native button tint should adapt to a dark host instead of inheriting black"
        )
        XCTAssertEqual(alpha, 0.46, accuracy: 0.01)
        XCTAssertNotEqual(
            tintColor, .systemGray,
            "Disabled native button should use transparent foreground instead of fixed system gray"
        )
        XCTAssertEqual(toolbar.firstButtonTitleColorForTesting(.disabled), tintColor)
    }

    func testAccessoryToolbarNativeEnabledButtonInheritsSystemTintAtFullAlpha() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        toolbar.applyBoldStateForTesting(active: false, enabled: true)

        XCTAssertEqual(
            toolbar.firstButtonAlphaForTesting, 1.0, accuracy: 0.01,
            "Enabled native button must be at full alpha"
        )
        XCTAssertNotEqual(
            toolbar.firstButtonTintColorForTesting, .systemGray,
            "Enabled native button must not use the disabled .systemGray tint"
        )
    }

    func testAccessoryToolbarAppliesNativeAppearanceToMentionSuggestions() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        _ = toolbar.setMentionSuggestions([
            NativeMentionSuggestion(dictionary: [
                "key": "alice",
                "title": "Alice Chen",
                "subtitle": "Design",
                "label": "@alice",
                "attrs": ["label": "@alice"]
            ])!
        ])

        XCTAssertTrue(toolbar.mentionButtonAtForTesting(0)?.usesNativeAppearanceForTesting() == true)
    }

    func testAccessoryToolbarNativeMentionSuggestionsUseNativeGlassTextRendering() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        _ = toolbar.setMentionSuggestions([
            NativeMentionSuggestion(dictionary: [
                "key": "alice",
                "title": "Alice Chen",
                "subtitle": "Design",
                "label": "@alice",
                "attrs": ["label": "@alice"]
            ])!
        ])

        #if compiler(>=6.2)
            if #available(iOS 26.0, *) {
                XCTAssertTrue(
                    toolbar.mentionButtonAtForTesting(0)?.usesNativeGlassTextRenderingForTesting() == true,
                    "Native mention suggestions should let UIKit render adaptive glass text"
                )
                XCTAssertTrue(
                    toolbar.mentionButtonAtForTesting(0)?.usesNativeGlassSemiboldTitleForTesting() == true,
                    "Native mention suggestions should keep the mention label semibold in glass"
                )
            }
        #endif
    }

    func testAccessoryToolbarNativeMentionSuggestionsUseTransparentOuterChrome() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        #if compiler(>=6.2)
            if #available(iOS 26.0, *) {
                XCTAssertFalse(toolbar.nativeChromeIsTransparentForTesting)

                _ = toolbar.setMentionSuggestions([
                    NativeMentionSuggestion(dictionary: [
                        "key": "alice",
                        "title": "Alice Chen",
                        "subtitle": "Design",
                        "label": "@alice",
                        "attrs": ["label": "@alice"]
                    ])!
                ])

                XCTAssertTrue(
                    toolbar.nativeChromeIsTransparentForTesting,
                    "Native mention chips own the glass surface, so the surrounding toolbar chrome should be transparent"
                )

                _ = toolbar.setMentionSuggestions([])

                XCTAssertFalse(
                    toolbar.nativeChromeIsTransparentForTesting,
                    "The native toolbar chrome should return when mention suggestions are cleared"
                )
            }
        #endif
    }

    func testAccessoryToolbarNativeMentionChromeTransitionAnimatesWhenHosted() {
        #if compiler(>=6.2)
            guard #available(iOS 26.0, *) else {
                return
            }

            let animationsWereEnabled = UIView.areAnimationsEnabled
            UIView.setAnimationsEnabled(true)
            defer {
                UIView.setAnimationsEnabled(animationsWereEnabled)
            }

            let toolbar = EditorAccessoryToolbarView(frame: CGRect(x: 0, y: 0, width: 320, height: 56))
            let window = makeTestWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
            let viewController = UIViewController()
            window.rootViewController = viewController
            window.makeKeyAndVisible()
            viewController.view.addSubview(toolbar)
            toolbar.layoutIfNeeded()
            defer {
                toolbar.removeFromSuperview()
                window.isHidden = true
            }

            toolbar.apply(theme: EditorToolbarTheme(dictionary: [
                "appearance": "native"
            ]))

            _ = toolbar.setMentionSuggestions([
                NativeMentionSuggestion(dictionary: [
                    "key": "alice",
                    "title": "Alice Chen",
                    "subtitle": "Design",
                    "label": "@alice",
                    "attrs": ["label": "@alice"]
                ])!
            ])

            XCTAssertTrue(toolbar.didAnimateChromeTransitionForTesting)
            XCTAssertFalse(
                toolbar.nativeChromeIsTransparentForTesting,
                "The outer chrome should fade out instead of disappearing immediately"
            )

            let expectation = expectation(description: "chrome transition completed")
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                expectation.fulfill()
            }
            wait(for: [expectation], timeout: 1.0)

            XCTAssertTrue(toolbar.nativeChromeIsTransparentForTesting)
        #endif
    }

    func testNativeMentionSuggestionFallbackTextTracksTintColor() {
        if #available(iOS 26.0, *) {
            return
        }

        let chip = MentionSuggestionChipButton(
            suggestion: NativeMentionSuggestion(dictionary: [
                "key": "alice",
                "title": "Alice Chen",
                "subtitle": "Design",
                "label": "@alice",
                "attrs": ["label": "@alice"]
            ])!,
            theme: nil,
            toolbarAppearance: .native
        )
        let tint = UIColor(red: 0.12, green: 0.34, blue: 0.56, alpha: 1)

        chip.tintColor = tint

        XCTAssertEqual(chip.titleTextColorForTesting(), tint)
        XCTAssertEqual(chip.subtitleTextColorForTesting(), tint.withAlphaComponent(0.72))
    }

    func testAccessoryToolbarNativeLayoutFittingPreservesVisibleHeight() {
        let toolbar = EditorAccessoryToolbarView(frame: CGRect(x: 0, y: 0, width: 320, height: 0))

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        toolbar.layoutIfNeeded()

        let fittedSize = toolbar.systemLayoutSizeFitting(
            CGSize(width: 320, height: UIView.layoutFittingCompressedSize.height)
        )
        XCTAssertGreaterThanOrEqual(fittedSize.height, 50, "native accessory toolbar should not collapse")
    }

    func testAccessoryToolbarNativeLayoutAllowsHorizontalOverflowScrolling() {
        let toolbar = EditorAccessoryToolbarView(frame: CGRect(x: 0, y: 0, width: 180, height: 56))

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        toolbar.layoutIfNeeded()

        XCTAssertGreaterThan(
            toolbar.nativeToolbarContentWidthForTesting,
            toolbar.nativeToolbarVisibleWidthForTesting,
            "native toolbar should overflow horizontally so all items remain reachable"
        )
        XCTAssertEqual(
            toolbar.nativeToolbarContentOffsetXForTesting,
            0,
            accuracy: 0.1,
            "native toolbar should start left-aligned"
        )
    }

    func testAccessoryToolbarNativeLayoutPreservesScrolledOffsetAcrossRelayout() {
        let toolbar = EditorAccessoryToolbarView(frame: CGRect(x: 0, y: 0, width: 180, height: 56))

        toolbar.apply(theme: EditorToolbarTheme(dictionary: [
            "appearance": "native"
        ]))
        toolbar.layoutIfNeeded()

        let targetOffset = min(40, toolbar.nativeToolbarContentWidthForTesting - toolbar.nativeToolbarVisibleWidthForTesting)
        XCTAssertGreaterThan(targetOffset, 0)
        toolbar.setNativeToolbarContentOffsetXForTesting(targetOffset)
        toolbar.layoutIfNeeded()
        XCTAssertEqual(
            toolbar.nativeToolbarContentOffsetXForTesting,
            targetOffset,
            accuracy: 0.1,
            "native toolbar should not snap back after relayout"
        )
    }

}
