import CoreText
import XCTest

extension RenderBridgeTests {
    func testRender_ruleFreeMentionMatchesLegacyProjection() throws {
        let styles: [String: [String: Any]] = ["mention": ["paddingLeft": 3, "fontSize": 21, "color": "#ff0000ff"]]
        let root: [String: Any] = ["mentions": ["node": ["style": ["paddingRight": 9]]]]
        let versioned = EditorTheme(dictionary: root.merging(["version": 1, "styles": styles]) { _, new in new })
        let legacy = EditorTheme(dictionary: EditorTheme.legacyProjection(styles: styles, root: root))
        func render(_ theme: EditorTheme) -> NSAttributedString {
            RenderBridge.attributedStringForOpaqueInlineAtom(nodeType: "mention", label: "same", docPos: 0, baseFont: baseFont, textColor: textColor, blockStack: [], topLevelChildIndex: nil, theme: theme, mentionTheme: nil)
        }
        let current = render(versioned)
        let previous = render(legacy)
        let currentBox = try XCTUnwrap(current.attribute(editorMentionBoxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        let previousBox = try XCTUnwrap(previous.attribute(editorMentionBoxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        XCTAssertEqual(currentBox.size, previousBox.size)
        XCTAssertTrue(NSDictionary(dictionary: currentBox.box.values).isEqual(to: previousBox.box.values))
        XCTAssertEqual(current.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor, previous.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor)
    }

    func testRender_contentAndPlaceholderUseEmptyAncestry() {
        let view = EditorTextView(frame: .zero, textContainer: nil)
        view.placeholder = "Write"
        view.theme = EditorTheme(dictionary: [
            "version": 1, "styles": [:], "rules": [
                ["path": ["content"], "style": ["paddingLeft": 23]],
                ["path": ["placeholder"], "style": ["color": "#ff0000ff"]],
                ["path": ["paragraph", "placeholder"], "style": ["color": "#00ff00ff"]]
            ]
        ])
        XCTAssertEqual(view.textContainerInset.left, 23)
        XCTAssertEqual(view.placeholderLabel.attributedText?.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor, .red)
    }

    func testRender_contextualMentionPreservesLocalOverrides() throws {
        let theme = EditorTheme(dictionary: [
            "version": 1, "styles": ["mention": ["paddingLeft": 3, "color": "#ff0000ff"]],
            "mentions": ["node": ["style": ["paddingRight": 9]]],
            "rules": [["path": ["paragraph", "mention"], "style": ["paddingLeft": 17, "paddingRight": 21, "color": "#00ff00ff"]]]
        ])
        let result = RenderBridge.renderElements(fromJSON: """
        [
            {"type":"blockStart","nodeType":"paragraph","depth":0},
            {"type":"opaqueInlineAtom","nodeType":"mention","docPos":0,"label":"rule"},
            {"type":"opaqueInlineAtom","nodeType":"mention","docPos":1,"label":"local","mentionTheme":{"node":{"style":{"paddingLeft":5}}}},
            {"type":"blockEnd"}
        ]
        """, baseFont: baseFont, textColor: textColor, theme: theme)
        let first = try XCTUnwrap(result.attribute(editorMentionBoxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        let local = try XCTUnwrap(result.attribute(editorMentionBoxAttribute, at: 4, effectiveRange: nil) as? EditorMentionRenderedBox)
        XCTAssertEqual(first.box.padding.left, 17)
        XCTAssertEqual(first.box.padding.right, 9)
        XCTAssertEqual(result.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor, .green)
        XCTAssertEqual(local.box.padding.left, 5)
    }

    func testRender_contextualSpecialElements() throws {
        let theme = EditorTheme(dictionary: [
            "version": 1,
            "styles": ["taskCheckbox": ["size": 20, "checked": ["size": 26]], "image": ["paddingLeft": 2]],
            "rules": [
                ["path": ["taskItem", "taskCheckbox"], "style": ["size": 30, "checked": ["size": 34]]],
                ["path": ["paragraph", "bold"], "style": ["color": "#ff0000ff"]],
                ["path": ["blockquote", "horizontalRule"], "style": ["marginTop": 19, "height": 7]],
                ["path": ["blockquote", "image"], "style": ["paddingLeft": 11, "resizeMode": "contain"]]
            ]
        ])
        let result = RenderBridge.renderElements(fromJSON: """
        [
            {"type":"blockStart","nodeType":"blockquote","depth":0},
            {"type":"blockStart","nodeType":"taskItem","depth":1,"listContext":{"kind":"task","checked":true,"isFirst":true,"isLast":true}},
            {"type":"blockStart","nodeType":"paragraph","depth":1},
            {"type":"textRun","text":"marked","marks":["bold"]},
            {"type":"blockEnd"},{"type":"blockEnd"},
            {"type":"voidBlock","nodeType":"horizontalRule","docPos":20},
            {"type":"voidBlock","nodeType":"image","docPos":21,"attrs":{"src":"invalid","width":40,"height":20}},
            {"type":"blockEnd"}
        ]
        """, baseFont: baseFont, textColor: textColor, theme: theme)
        let checkbox = try XCTUnwrap(result.attribute(editorTaskCheckboxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        XCTAssertEqual(checkbox.box.number("size"), 34)
        XCTAssertEqual(result.attribute(RenderBridgeAttributes.listMarkerWidth, at: 0, effectiveRange: nil) as? CGFloat, 42)
        XCTAssertEqual(result.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor, .red)
        var rule: HorizontalRuleAttachment?
        var image: BlockImageAttachment?
        result.enumerateAttribute(.attachment, in: NSRange(location: 0, length: result.length)) { value, _, _ in
            if let value = value as? HorizontalRuleAttachment { rule = value }
            if let value = value as? BlockImageAttachment { image = value }
        }
        XCTAssertEqual(rule?.styleBox?.margin.top, 19)
        XCTAssertEqual(rule?.lineHeight, 7)
        XCTAssertEqual(image?.styleBox?.padding.left, 11)
        XCTAssertEqual(image?.styleBox?.values["resizeMode"] as? String, "contain")
    }

    func testStyleSheetContextualBoxRulesPreserveSparseValuesAndZeros() throws {
        let theme = EditorTheme(dictionary: [
            "version": 1,
            "styles": ["paragraph": ["marginBottom": 8, "paddingLeft": 3]],
            "rules": [["path": ["listItem", "paragraph"], "style": ["marginBottom": 0]]]
        ])
        let sheet = try XCTUnwrap(theme.styleSheet)
        XCTAssertEqual(sheet.box("paragraph").margin.bottom, 8)
        XCTAssertEqual(sheet.box("paragraph", ancestors: ["list_item"]).margin.bottom, 0)
        XCTAssertEqual(sheet.box("paragraph", ancestors: ["list_item"]).padding.left, 3)
        XCTAssertEqual(sheet["paragraph"]["marginBottom"] as? Int, 8)
    }

    func testStyleSheetRulesMatchOnlyContiguousCanonicalSuffixes() throws {
        let cases: [(path: [String], ancestors: [String], matches: Bool)] = [
            (["paragraph"], [], true),
            (["paragraph"], ["blockquote", "bullet_list", "list_item"], true),
            (["listItem", "paragraph"], ["blockquote", "bullet_list", "list_item"], true),
            (["bulletList", "listItem", "paragraph"], ["blockquote", "bullet_list", "list_item"], true),
            (["blockquote", "listItem", "paragraph"], ["blockquote", "list_item"], true),
            (["blockquote", "listItem", "paragraph"], ["blockquote", "bullet_list", "list_item"], false),
            (["blockquote", "bulletList", "listItem", "paragraph"], ["bullet_list", "list_item"], false),
            (["listItem", "paragraph"], ["blockquote"], false),
            (["list_item", "paragraph"], ["listItem"], true)
        ]
        for (index, test) in cases.enumerated() {
            let sheet = try XCTUnwrap(EditorTheme(dictionary: [
                "version": 1,
                "styles": ["paragraph": ["marginBottom": 8]],
                "rules": [["path": test.path, "style": ["marginBottom": 17]]]
            ]).styleSheet)
            XCTAssertEqual(sheet.box("paragraph", ancestors: test.ancestors).margin.bottom,
                test.matches ? 17 : 8, "case \(index)")
        }
    }

    func testStyleSheetRulesUseDeclarationOrderAndAccumulateSparseValues() throws {
        let sheet = try XCTUnwrap(EditorTheme(dictionary: [
            "version": 1,
            "styles": ["paragraph": ["marginBottom": 8, "paddingLeft": 3]],
            "rules": [
                ["path": ["listItem", "paragraph"], "style": ["marginBottom": 2, "paddingLeft": 9]],
                ["path": ["paragraph"], "style": ["marginBottom": 1]]
            ]
        ]).styleSheet)
        let box = sheet.box("paragraph", ancestors: ["list_item"])
        XCTAssertEqual(box.margin.bottom, 1)
        XCTAssertEqual(box.padding.left, 9)
        XCTAssertEqual(sheet.resolvedValues("paragraph", ancestors: ["list_item"])["paddingLeft"] as? Int, 9)
        XCTAssertEqual(sheet["paragraph"]["paddingLeft"] as? Int, 3)
    }

    func testStyleSheetResolvedValuesPreserveSparseSpecialProperties() throws {
        let sheet = try XCTUnwrap(EditorTheme(dictionary: [
            "version": 1,
            "styles": [
                "bulletList": ["indent": 24, "baseIndentMultiplier": 2],
                "listMarker": ["scale": 0.8, "gap": 6, "ordered": ["schemes": ["upperAlpha"], "suffix": "."]],
                "taskCheckbox": ["size": 20, "gap": 4, "checkColor": "#ffffffff",
                    "checked": ["backgroundColor": "#ff0000ff", "borderRightWidth": 3]],
                "image": ["resizeMode": "cover", "paddingLeft": 5],
                "horizontalRule": ["height": 2]
            ],
            "rules": [
                ["path": ["blockquote", "bulletList"], "style": ["indent": 0, "baseIndentMultiplier": 0]],
                ["path": ["listMarker"], "style": ["scale": 0, "gap": 0, "ordered": ["suffix": ")"]]],
                ["path": ["taskCheckbox"], "style": ["size": 0, "gap": 0, "checkColor": "#00000000",
                    "checked": ["backgroundColor": "#00ff00ff", "borderLeftWidth": 0]]],
                ["path": ["taskCheckbox"], "style": ["checked": ["borderTopWidth": 2]]],
                ["path": ["image"], "style": ["resizeMode": "stretch"]],
                ["path": ["horizontalRule"], "style": ["height": 0]]
            ]
        ]).styleSheet)
        let list = sheet.resolvedValues("bullet_list", ancestors: ["blockquote"])
        XCTAssertEqual(list["indent"] as? Int, 0)
        XCTAssertEqual(list["baseIndentMultiplier"] as? Int, 0)
        let marker = sheet.resolvedValues("listMarker")
        XCTAssertEqual(marker["scale"] as? Int, 0)
        XCTAssertEqual(marker["gap"] as? Int, 0)
        let ordered = try XCTUnwrap(marker["ordered"] as? [String: Any])
        XCTAssertEqual(ordered["suffix"] as? String, ")")
        XCTAssertEqual(ordered["schemes"] as? [String], ["upperAlpha"])
        let checkbox = sheet.resolvedValues("taskCheckbox")
        XCTAssertEqual(checkbox["size"] as? Int, 0)
        XCTAssertEqual(checkbox["gap"] as? Int, 0)
        XCTAssertEqual(checkbox["checkColor"] as? String, "#00000000")
        let checked = try XCTUnwrap(checkbox["checked"] as? [String: Any])
        XCTAssertEqual(checked["backgroundColor"] as? String, "#00ff00ff")
        XCTAssertEqual(checked["borderRightWidth"] as? Int, 3)
        XCTAssertEqual(checked["borderLeftWidth"] as? Int, 0)
        XCTAssertEqual(checked["borderTopWidth"] as? Int, 2)
        XCTAssertEqual(sheet.resolvedValues("image")["resizeMode"] as? String, "stretch")
        XCTAssertEqual(sheet.box("image").padding.left, 5)
        XCTAssertEqual(sheet.resolvedValues("horizontal_rule")["height"] as? Int, 0)
        XCTAssertNil(sheet.resolvedValues("horizontal_rule")["paddingLeft"])
    }

    func testStyleSheetEmptyUnmatchedAndRuleOnlyStyles() throws {
        let styles: [String: [String: Any]] = ["image": ["resizeMode": "cover", "paddingLeft": 5]]
        let rules: [[[String: Any]]] = [
            [],
            [["path": ["image"], "style": [:]]],
            [["path": ["blockquote", "image"], "style": ["resizeMode": "stretch"]]]
        ]
        for ruleSet in rules {
            let sheet = try XCTUnwrap(EditorTheme(dictionary: [
                "version": 1, "styles": styles, "rules": ruleSet
            ]).styleSheet)
            XCTAssertTrue(NSDictionary(dictionary: sheet.resolvedValues("image")).isEqual(to: sheet["image"]))
            XCTAssertTrue(sheet.resolvedValues("paragraph").isEmpty)
            XCTAssertEqual(sheet.box("paragraph", ancestors: ["list_item"]).margin.bottom, 8)
            XCTAssertEqual(sheet.box("list_item", ancestors: ["bullet_list"]).margin.bottom, 4)
        }
        let ruleOnly = try XCTUnwrap(EditorTheme(dictionary: [
            "version": 1, "rules": [["path": ["image"], "style": ["resizeMode": "stretch"]]]
        ]).styleSheet)
        XCTAssertEqual(ruleOnly.resolvedValues("image")["resizeMode"] as? String, "stretch")
        XCTAssertTrue(ruleOnly["image"].isEmpty)
    }

    func testStyleSheetMarkRulesResolveWithinExistingCascadeSlots() throws {
        let sheet = try XCTUnwrap(EditorTheme(dictionary: [
            "version": 1,
            "styles": [
                "bold": ["color": "#220000ff"],
                "link": ["color": "#550000ff"]
            ],
            "rules": [["path": ["listItem", "paragraph", "strong"], "style": [
                "color": "#440000ff", "fontWeight": "normal", "fontSize": 20, "letterSpacing": 0
            ]]]
        ]).styleSheet)
        let base: [NSAttributedString.Key: Any] = [.font: baseFont, .foregroundColor: UIColor.black, .kern: 3]
        let ancestors = ["list_item", "paragraph"]
        let marked = sheet.inlineAttributes(["strong"], base: base, scale: 1.5, ancestors: ancestors)
        XCTAssertEqual(marked[.foregroundColor] as? UIColor, EditorTheme.color(from: "#440000ff"))
        XCTAssertEqual((marked[.font] as? UIFont)?.pointSize, 30)
        XCTAssertFalse((marked[.font] as? UIFont)?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? true)
        XCTAssertEqual(marked[.kern] as? CGFloat, 0)
        let linked = sheet.inlineAttributes([["type": "link", "href": "https://example.com"], "strong"],
            base: base, ancestors: ancestors)
        XCTAssertEqual(linked[.foregroundColor] as? UIColor, EditorTheme.color(from: "#550000ff"))
        XCTAssertEqual(linked[RenderBridgeAttributes.linkHref] as? String, "https://example.com")
        let unscoped = sheet.inlineAttributes(["strong"], base: base)
        XCTAssertEqual(unscoped[.foregroundColor] as? UIColor, EditorTheme.color(from: "#220000ff"))
        XCTAssertTrue((unscoped[.font] as? UIFont)?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? false)
        XCTAssertEqual(sheet.textStyle("strong", ancestors: ancestors).color, marked[.foregroundColor] as? UIColor)
    }

    func testStyleSheetTargetRulesFollowElementTextWithoutRecursiveInheritance() throws {
        let sheet = try XCTUnwrap(EditorTheme(dictionary: [
            "version": 1,
            "styles": [
                "text": ["fontFamily": "Courier", "letterSpacing": 2, "textDecorationStyle": "double"],
                "blockquote": ["fontSize": 23, "textAlign": "left"],
                "paragraph": ["color": "#110000ff", "textDecorationLine": "underline"]
            ],
            "rules": [
                ["path": ["listItem", "paragraph"], "style": [
                    "color": "#330000ff", "lineHeight": 0, "letterSpacing": 0,
                    "textAlign": "right", "textDecorationLine": "none", "textDecorationColor": "#440000ff"
                ]],
                ["path": ["blockquote"], "style": ["fontSize": 40, "textAlign": "center"]]
            ]
        ]).styleSheet)
        let text = sheet.textStyle("paragraph", ancestors: ["blockquote", "list_item"],
            semantic: EditorTextStyle(fontSize: 18, fontWeight: "600"))
        XCTAssertEqual(text.fontFamily, "Courier")
        XCTAssertEqual(text.fontSize, 23)
        XCTAssertEqual(text.fontWeight, "600")
        XCTAssertEqual(text.color, EditorTheme.color(from: "#330000ff"))
        XCTAssertEqual(text.lineHeight, 0)
        let values = sheet.textValues("paragraph", ancestors: ["blockquote", "list_item"])
        XCTAssertEqual(values["letterSpacing"] as? Int, 0)
        XCTAssertEqual(values["textAlign"] as? String, "right")
        XCTAssertEqual(values["textDecorationLine"] as? String, "none")
        XCTAssertEqual(values["textDecorationColor"] as? String, "#440000ff")
        XCTAssertEqual(values["textDecorationStyle"] as? String, "double")
        XCTAssertNil(values["lineHeight"])
        XCTAssertEqual(sheet.textValues("paragraph", ancestors: ["blockquote"])["textAlign"] as? String, "left")
        XCTAssertEqual(sheet.textStyle("blockquote").fontSize, 40)
    }

    func testStyleSheetMalformedRulesPreserveBaseStylesAndValidSiblings() throws {
        for field in ["{}", "null", "3", "\"invalid\""] {
            let theme = try XCTUnwrap(EditorTheme.from(json:
                "{\"version\":1,\"styles\":{\"paragraph\":{\"marginBottom\":8}},\"rules\":\(field)}"))
            XCTAssertEqual(theme.styleSheet?.box("paragraph").margin.bottom, 8)
        }
        let sheet = try XCTUnwrap(EditorTheme.from(json: """
        {
            "version": 1,
            "styles": {"paragraph": {"marginBottom": 8, "paddingLeft": 3}},
            "rules": [
                {"path": ["paragraph"], "style": {"marginBottom": 2}},
                null,
                "invalid",
                {"style": {"marginBottom": 9}},
                {"path": [], "style": {"marginBottom": 7}},
                {"path": ["unknown"], "style": {"marginBottom": 6}},
                {"path": [1, "paragraph"], "style": {"marginBottom": 5}},
                {"path": ["paragraph"], "style": []},
                {"path": ["paragraph"]},
                {"path": ["paragraph"], "style": {"paddingRight": 4}}
            ]
        }
        """)?.styleSheet)
        XCTAssertEqual(sheet.box("paragraph").margin.bottom, 2)
        XCTAssertEqual(sheet.box("paragraph").padding.left, 3)
        XCTAssertEqual(sheet.box("paragraph").padding.right, 4)
        XCTAssertEqual(sheet.box("unknown").margin.bottom, 0)
    }

    func testStyleSheetCompatibilityDefaultsAndExplicitZeros() {
        let defaults = EditorStyleSheet(styles: [:])
        XCTAssertEqual(defaults.box("paragraph").margin.bottom, 8)
        XCTAssertEqual(defaults.box("listItem").margin.bottom, 4)

        let theme = EditorTheme(dictionary: [
            "version": 1,
            "styles": [
                "paragraph": [
                    "marginTop": 0,
                    "marginRight": 7,
                    "marginBottom": 0,
                    "marginLeft": 5,
                    "paddingTop": 3,
                    "paddingRight": 4,
                    "paddingBottom": 0,
                    "paddingLeft": 2,
                    "borderTopWidth": 1,
                    "borderRightWidth": 0,
                    "borderBottomWidth": 6,
                    "borderLeftWidth": 0
                ]
            ]
        ])
        let box = theme.styleSheet!.box("paragraph")
        XCTAssertEqual(box.margin, UIEdgeInsets(top: 0, left: 5, bottom: 0, right: 7))
        XCTAssertEqual(box.padding, UIEdgeInsets(top: 3, left: 2, bottom: 0, right: 4))
        XCTAssertEqual(box.borders, UIEdgeInsets(top: 1, left: 0, bottom: 6, right: 0))
    }

    func testStyleSheetCompatibilityPreservesInheritedTextCascade() {
        let theme = EditorTheme(dictionary: [
            "version": 1,
            "styles": [
                "text": [
                    "fontFamily": "Courier",
                    "fontSize": 19,
                    "lineHeight": 28,
                    "color": "#11223380"
                ],
                "blockquote": ["fontSize": 21],
                "paragraph": ["fontWeight": "700", "letterSpacing": 0]
            ]
        ])
        let sheet = theme.styleSheet!
        let text = sheet.textStyle("paragraph", ancestors: ["blockquote"])

        XCTAssertEqual(text.fontFamily, "Courier")
        XCTAssertEqual(text.fontSize, 21)
        XCTAssertEqual(text.fontWeight, "700")
        XCTAssertEqual(
            text.color,
            UIColor(
                red: 0x11 as CGFloat / 255.0,
                green: 0x22 as CGFloat / 255.0,
                blue: 0x33 as CGFloat / 255.0,
                alpha: 0x80 as CGFloat / 255.0
            )
        )
        XCTAssertEqual(text.lineHeight, 28)
        XCTAssertEqual(
            sheet.textValues("paragraph", ancestors: ["blockquote"])["letterSpacing"] as? Int,
            0
        )
    }

    func testRender_opaqueInlineAtom() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "before ", "marks": []},
            {"type": "opaqueInlineAtom", "label": "widget", "docPos": 8},
            {"type": "textRun", "text": " after", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor
        )

        XCTAssertTrue(
            result.string.contains("[widget]"),
            "Opaque inline atom should render as '[widget]'. Got: '\(result.string)'"
        )
    }

    func testRender_mentionInlineAtomUsesVisibleLabelAndTheme() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "Hello ", "marks": []},
            {"type": "opaqueInlineAtom", "nodeType": "mention", "label": "@Alice", "docPos": 7},
            {"type": "textRun", "text": "!", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "mentions": [
                "node": [
                    "textColor": "#112233",
                    "backgroundColor": "#ddeeff",
                    "fontWeight": "bold"
                ]
            ]
        ])
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        XCTAssertTrue(
            result.string.contains("@Alice"),
            "Mention inline atom should render its visible label. Got: '\(result.string)'"
        )
        XCTAssertFalse(
            result.string.contains("[@Alice]"),
            "Mention inline atom should not render using generic opaque brackets. Got: '\(result.string)'"
        )

        let mentionRange = (result.string as NSString).range(of: "@Alice")
        XCTAssertNotEqual(mentionRange.location, NSNotFound)

        let attrs = result.attributes(at: mentionRange.location, effectiveRange: nil)
        XCTAssertEqual(
            attrs[.foregroundColor] as? UIColor,
            UIColor(
                red: 0x11 as CGFloat / 255.0,
                green: 0x22 as CGFloat / 255.0,
                blue: 0x33 as CGFloat / 255.0,
                alpha: 1.0
            )
        )
        XCTAssertEqual(
            attrs[.backgroundColor] as? UIColor,
            UIColor(
                red: 0xdd as CGFloat / 255.0,
                green: 0xee as CGFloat / 255.0,
                blue: 0xff as CGFloat / 255.0,
                alpha: 1.0
            )
        )
        let font = attrs[.font] as? UIFont
        XCTAssertTrue(
            font?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? false,
            "Mention theme should be able to request a bold font"
        )
    }

    func testRender_mentionInlineAtomMergesElementMentionThemeOverride() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {
                "type": "opaqueInlineAtom",
                "nodeType": "mention",
                "label": "@Alice",
                "docPos": 1,
                "mentionTheme": {"node": {"textColor": "#445566"}}
            },
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "mentions": [
                "node": [
                    "textColor": "#112233",
                    "backgroundColor": "#ddeeff",
                    "fontWeight": "bold"
                ]
            ]
        ])
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        XCTAssertEqual(result.string, "@Alice")

        let attrs = result.attributes(at: 0, effectiveRange: nil)
        XCTAssertEqual(
            attrs[.foregroundColor] as? UIColor,
            UIColor(
                red: 0x44 as CGFloat / 255.0,
                green: 0x55 as CGFloat / 255.0,
                blue: 0x66 as CGFloat / 255.0,
                alpha: 1.0
            )
        )
        XCTAssertEqual(
            attrs[.backgroundColor] as? UIColor,
            UIColor(
                red: 0xdd as CGFloat / 255.0,
                green: 0xee as CGFloat / 255.0,
                blue: 0xff as CGFloat / 255.0,
                alpha: 1.0
            )
        )
        let font = attrs[.font] as? UIFont
        XCTAssertTrue(
            font?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? false,
            "Mention override should preserve global bold styling. Got: \(String(describing: font))"
        )
    }

    func testRender_opaqueBlockAtom() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "Above", "marks": []},
            {"type": "blockEnd"},
            {"type": "opaqueBlockAtom", "label": "codeBlock", "docPos": 7}
        ]
        """
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor
        )

        XCTAssertTrue(
            result.string.contains("[codeBlock]"),
            "Opaque block atom should render as '[codeBlock]'. Got: '\(result.string)'"
        )
    }

    func testRender_themeOverridesParagraphTypography() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "Styled", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "text": [
                "fontFamily": "Courier",
                "fontSize": 18,
                "color": "#112233"
            ],
            "paragraph": [
                "lineHeight": 28,
                "spacingAfter": 14
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let attrs = result.attributes(at: 0, effectiveRange: nil)
        let font = attrs[.font] as? UIFont
        let color = attrs[.foregroundColor] as? UIColor
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(font?.pointSize ?? 0, 18, accuracy: 0.1)
        XCTAssertEqual(color, EditorTheme.color(from: "#112233"))
        XCTAssertEqual(paragraphStyle?.minimumLineHeight ?? 0, 28, accuracy: 0.1)
        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? 0, 14, accuracy: 0.1)
    }

    func testRender_themeOverridesSpecificHeadingLevelTypography() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "h2", "depth": 0},
            {"type": "textRun", "text": "Section title", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "text": [
                "fontSize": 16,
                "color": "#112233"
            ],
            "headings": [
                "h2": [
                    "fontSize": 28,
                    "fontWeight": "700",
                    "color": "#445566",
                    "lineHeight": 34,
                    "spacingAfter": 12
                ],
                "h4": [
                    "fontSize": 18,
                    "color": "#AA5500"
                ]
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let attrs = result.attributes(at: 0, effectiveRange: nil)
        let font = attrs[.font] as? UIFont
        let color = attrs[.foregroundColor] as? UIColor
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(font?.pointSize ?? 0, 28, accuracy: 0.1)
        XCTAssertTrue(
            font?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? false,
            "Configured h2 heading should resolve to a bold font"
        )
        XCTAssertEqual(color, EditorTheme.color(from: "#445566"))
        XCTAssertEqual(paragraphStyle?.minimumLineHeight ?? 0, 34, accuracy: 0.1)
        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? 0, 12, accuracy: 0.1)
    }

    func testRender_listItemUsesListItemSpacingWhenParagraphSpacingUnset() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 2, "start": 1, "isFirst": true, "isLast": false}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "First item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 2, "total": 2, "start": 1, "isFirst": false, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Second item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "list": [
                "itemSpacing": 14
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let attrs = result.attributes(at: 0, effectiveRange: nil)
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? 0, 14, accuracy: 0.1)
    }

    func testRender_listItemSpacingOverridesParagraphSpacingForSiblingListItems() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 2, "start": 1, "isFirst": true, "isLast": false}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "First item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 2, "total": 2, "start": 1, "isFirst": false, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Second item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "spacingAfter": 14
            ],
            "list": [
                "itemSpacing": 6
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let nsString = result.string as NSString
        let firstRange = nsString.range(of: "First item")
        XCTAssertNotEqual(firstRange.location, NSNotFound)

        let attrs = result.attributes(at: firstRange.location, effectiveRange: nil)
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? -1, 6, accuracy: 0.1)
    }

    func testRender_nestedListSpacingAfter() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 2, "start": 1, "isFirst": true, "isLast": false}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "First item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 2, "total": 2, "start": 1, "isFirst": false, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Parent item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Nested item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "After nested", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "After list", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "list": [
                "itemSpacing": 6,
                "spacingAfter": 20
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )
        let text = result.string as NSString
        let firstStyle = result.attribute(
            .paragraphStyle,
            at: text.range(of: "First item").location,
            effectiveRange: nil
        ) as? NSParagraphStyle
        let nestedStyle = result.attribute(
            .paragraphStyle,
            at: text.range(of: "Nested item").location,
            effectiveRange: nil
        ) as? NSParagraphStyle
        let outerFinalStyle = result.attribute(
            .paragraphStyle,
            at: text.range(of: "After nested").location,
            effectiveRange: nil
        ) as? NSParagraphStyle

        XCTAssertEqual(firstStyle?.paragraphSpacing ?? -1, 6, accuracy: 0.1)
        XCTAssertEqual(nestedStyle?.paragraphSpacing ?? -1, 20, accuracy: 0.1)
        XCTAssertEqual(outerFinalStyle?.paragraphSpacing ?? -1, 20, accuracy: 0.1)
    }

    func testRender_stackedNestedListSpacingAfter() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Parent item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Nested item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "After list", "marks": []},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "list": [
                "itemSpacing": 6,
                "spacingAfter": 20
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )
        let text = result.string as NSString
        let nestedStyle = result.attribute(
            .paragraphStyle,
            at: text.range(of: "Nested item").location,
            effectiveRange: nil
        ) as? NSParagraphStyle

        XCTAssertEqual(nestedStyle?.paragraphSpacing ?? -1, 40, accuracy: 0.1)
    }

    func testRender_nestedFirstListItemDoesNotKeepParentParagraphSpacingWhenItemSpacingIsZero() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Parent item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Nested item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "spacingAfter": 14
            ],
            "list": [
                "itemSpacing": 0
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let nsString = result.string as NSString
        let parentRange = nsString.range(of: "Parent item")
        XCTAssertNotEqual(parentRange.location, NSNotFound)

        let attrs = result.attributes(at: parentRange.location, effectiveRange: nil)
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? -1, 0, accuracy: 0.1)
    }

    func testRender_nestedSiblingListItemsUseListItemSpacingInsteadOfParagraphSpacing() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 0,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Parent item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 2, "start": 1, "isFirst": true, "isLast": false}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Child one", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 2, "total": 2, "start": 1, "isFirst": false, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Child two", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "spacingAfter": 14
            ],
            "list": [
                "itemSpacing": 6
            ]
        ])

        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: theme
        )

        let nsString = result.string as NSString
        let childRange = nsString.range(of: "Child one")
        XCTAssertNotEqual(childRange.location, NSNotFound)

        let attrs = result.attributes(at: childRange.location, effectiveRange: nil)
        let paragraphStyle = attrs[.paragraphStyle] as? NSParagraphStyle

        XCTAssertEqual(paragraphStyle?.paragraphSpacing ?? -1, 6, accuracy: 0.1)
    }

}
