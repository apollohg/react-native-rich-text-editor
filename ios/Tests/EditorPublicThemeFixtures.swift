import XCTest

struct EditorPublicThemeFixtures {
    let publicThemeJSON: String

    static func load() throws -> EditorPublicThemeFixtures {
        let url = try XCTUnwrap(Bundle(for: RichTextEditorViewTests.self)
            .url(forResource: "editor-public-theme-fixtures", withExtension: "json"))
        let raw = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let theme = try XCTUnwrap(raw["theme"] as? [String: Any])
        let themeData = try JSONSerialization.data(withJSONObject: theme)
        return EditorPublicThemeFixtures(publicThemeJSON: try XCTUnwrap(String(data: themeData, encoding: .utf8)))
    }
}
