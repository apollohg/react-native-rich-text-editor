import UIKit

final class ToolbarImageLoader {
    static let maximumDecodeDimension = 256
    private static let maximumSourceBytes = 2 * 1024 * 1024
    private static let policy = ImageLoadingPolicy(
        maxSourceBytes: maximumSourceBytes,
        connectTimeout: ImageLoadingPolicy.default.connectTimeout,
        readTimeout: ImageLoadingPolicy.default.readTimeout,
        requestTimeout: ImageLoadingPolicy.default.requestTimeout,
        maxConcurrentRequests: ImageLoadingPolicy.default.maxConcurrentRequests,
        maxPendingRequests: ImageLoadingPolicy.default.maxPendingRequests,
        maxDecodeDimension: maximumDecodeDimension
    )
    private let remote = RenderImageLoadOwner(policy: policy)
    private let localQueue: OperationQueue = {
        let queue = OperationQueue()
        queue.name = "com.apollohg.editor.toolbar-images"
        queue.maxConcurrentOperationCount = ImageLoadingPolicy.default.maxConcurrentRequests
        queue.qualityOfService = .userInitiated
        return queue
    }()
    private var generation = UUID()
    private var requested = Set<ToolbarImageSource>()
    private var images: [ToolbarImageSource: UIImage] = [:]
    var onImageLoaded: (() -> Void)?

    deinit {
        remote.cancelAll()
        localQueue.cancelAllOperations()
    }

    func reset() {
        generation = UUID()
        remote.cancelAll()
        localQueue.cancelAllOperations()
        requested.removeAll()
        images.removeAll()
    }

    func image(for source: ToolbarImageSource) -> UIImage? { images[source] }

    func load(_ source: ToolbarImageSource) {
        guard requested.insert(source).inserted else { return }
        let requestGeneration = generation
        let complete: (UIImage?) -> Void = { [weak self] image in
            guard let self, generation == requestGeneration, let image else { return }
            images[source] = image
            onImageLoaded?()
        }
        let url = URL(string: source.uri)
        if let scheme = url?.scheme?.lowercased(), ["http", "https", "data"].contains(scheme) {
            remote.loadImage(source: source.uri, completion: complete)
            return
        }
        localQueue.addOperation {
            let image: UIImage?
            if let url, url.isFileURL {
                image = Self.readLocalImage(url)
            } else if source.uri.hasPrefix("/") {
                image = Self.readLocalImage(URL(fileURLWithPath: source.uri))
            } else if url?.scheme == nil {
                // Release RN assets can resolve to a bundle-relative filename.
                let resource = source.uri.removingPercentEncoding ?? source.uri
                if let assetURL = Bundle.main.url(forResource: resource, withExtension: nil) {
                    image = Self.readLocalImage(assetURL)
                } else {
                    image = UIImage(named: resource)
                }
            } else {
                image = nil
            }
            DispatchQueue.main.async { complete(image) }
        }
    }

    private static func readLocalImage(_ url: URL) -> UIImage? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: maximumSourceBytes + 1),
              data.count <= maximumSourceBytes else { return nil }
        return DefaultImageDataDecoder().decode(data, maxDimension: maximumDecodeDimension)
    }

    static func rendered(_ image: UIImage, source: ToolbarImageSource, size: CGFloat, tintable: Bool) -> UIImage {
        let maximumSize = min(max(1, size), CGFloat(maximumDecodeDimension))
        let width = source.width.map { CGFloat($0) } ?? image.size.width
        let height = source.height.map { CGFloat($0) } ?? image.size.height
        let ratio = min(maximumSize / width, maximumSize / height)
        let target = CGSize(width: max(1, width * ratio), height: max(1, height * ratio))
        let format = UIGraphicsImageRendererFormat()
        format.scale = UIScreen.main.scale
        let resized = UIGraphicsImageRenderer(size: target, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: target))
        }
        return resized.withRenderingMode(tintable ? .alwaysTemplate : .alwaysOriginal)
    }
}
