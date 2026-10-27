import UIKit

extension EditorAccessoryToolbarView {
    func makeGroupMenu(item: NativeToolbarItem) -> UIMenu? {
        guard item.type == .group else { return nil }
        let actions = item.items.map { child in
            let state = buttonState(for: child, state: currentState)
            let image: UIImage?
            if let source = child.icon?.imageSource {
                imageLoader.load(source)
                image = imageLoader.image(for: source).map {
                    ToolbarImageLoader.rendered(
                        $0,
                        source: source,
                        size: resolvedButtonIconSize(for: child),
                        tintable: child.icon?.tintable ?? true
                    )
                }
            } else {
                image = child.icon?.resolvedSFSymbolName().flatMap { UIImage(systemName: $0) }
            }
            var title = child.label ?? child.icon?.resolvedGlyphText() ?? "Item"
            if child.icon?.imageSource != nil, image == nil {
                let fallback = child.icon?.resolvedGlyphText() ?? "?"
                if fallback != title {
                    title = "\(fallback) \(title)"
                }
            }
            return UIAction(
                title: title,
                image: image,
                identifier: nil,
                discoverabilityTitle: child.label,
                attributes: state.enabled ? [.keepsMenuPresented] : [.disabled],
                state: state.active ? .on : .off
            ) { [weak self] _ in
                self?.handleToolbarButtonPress(child)
            }
        }
        guard !actions.isEmpty else { return nil }
        return UIMenu(title: item.label ?? "", children: actions)
    }
}
