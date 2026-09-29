import UIKit

func makeTestWindow(frame: CGRect) -> UIWindow {
    let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
    precondition(scenes.count == 1, "NativeEditorTestHost must provide exactly one connected window scene")
    let window = UIWindow(windowScene: scenes[0])
    window.frame = frame
    return window
}
