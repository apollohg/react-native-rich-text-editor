import UIKit

final class AttributeLookupTextStorage: NSTextStorage {
    private let backing = NSMutableAttributedString(string: "")
    private(set) var absentMentionLongestLookupCount = 0
    private(set) var attributeLookupLocations: [Int] = []

    override var string: String { backing.string }

    override func attributes(at location: Int, effectiveRange range: NSRangePointer?) -> [NSAttributedString.Key: Any] {
        attributeLookupLocations.append(location)
        return backing.attributes(at: location, effectiveRange: range)
    }

    override func replaceCharacters(in range: NSRange, with string: String) {
        backing.replaceCharacters(in: range, with: string)
        edited(.editedCharacters, range: range, changeInLength: string.utf16.count - range.length)
    }

    override func setAttributes(_ attrs: [NSAttributedString.Key: Any]?, range: NSRange) {
        backing.setAttributes(attrs, range: range)
        edited(.editedAttributes, range: range, changeInLength: 0)
    }

    override func attribute(
        _ attrName: NSAttributedString.Key,
        at location: Int,
        longestEffectiveRange range: NSRangePointer?,
        in rangeLimit: NSRange
    ) -> Any? {
        let value = backing.attribute(attrName, at: location, longestEffectiveRange: range, in: rangeLimit)
        if attrName == editorMentionBoxAttribute, value == nil {
            absentMentionLongestLookupCount += 1
        }
        return value
    }

    func resetAttributeLookups() {
        attributeLookupLocations.removeAll(keepingCapacity: true)
    }
}
