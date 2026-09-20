import UIKit

struct TableStyle {
    var minColumnWidth: CGFloat = 80
    var cellPadding: CGFloat = 8
    var borderWidth: CGFloat = 1
    var borderColor: UIColor = EditorTheme.color(from: "#D1D5DB")!
    var headerBackgroundColor: UIColor = EditorTheme.color(from: "#F3F4F6")!
    var selectionColor: UIColor = EditorTheme.color(from: "#3B82F633")!
    var resizeHandleColor: UIColor = EditorTheme.color(from: "#3B82F6")!

    init(minColumnWidth: CGFloat = 80, cellPadding: CGFloat = 8, borderWidth: CGFloat = 1,
         borderColor: UIColor = EditorTheme.color(from: "#D1D5DB")!,
         headerBackgroundColor: UIColor = EditorTheme.color(from: "#F3F4F6")!,
         selectionColor: UIColor = EditorTheme.color(from: "#3B82F633")!,
         resizeHandleColor: UIColor = EditorTheme.color(from: "#3B82F6")!) {
        self.minColumnWidth = minColumnWidth
        self.cellPadding = cellPadding
        self.borderWidth = borderWidth
        self.borderColor = borderColor
        self.headerBackgroundColor = headerBackgroundColor
        self.selectionColor = selectionColor
        self.resizeHandleColor = resizeHandleColor
    }

    init?(theme: EditorTheme?) {
        self.init()
        guard let table = theme?.table else { return }
        guard table.isValid else { return nil }
        self = table
    }

    var isValid: Bool {
        minColumnWidth.isFinite && minColumnWidth > 0 && cellPadding.isFinite && cellPadding >= 0 &&
        borderWidth.isFinite && borderWidth >= 0
    }
}
