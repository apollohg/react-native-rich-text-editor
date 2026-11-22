import UIKit

@objc(NativeEditorFocusHost)
public final class NativeEditorFocusHost: NSObject {
    @objc(blurEditorBesideHost:)
    public static func blurEditor(besideHost host: UIView) {
        host.superview?.subviews
            .lazy
            .compactMap { $0 as? NativeEditorExpoView }
            .first?
            .blur()
    }
}
