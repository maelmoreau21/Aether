import SwiftUI
import UIKit

/// Poster URLs are versioned (`?v=…`): once fetched, an image never changes.
/// They need the token, so `AsyncImage` (no headers) is not enough.
final class PosterCache {
    static let shared = PosterCache()
    private let memory = NSCache<NSURL, UIImage>()
    private let session: URLSession = {
        let configuration = URLSessionConfiguration.default
        configuration.urlCache = URLCache(memoryCapacity: 32 << 20, diskCapacity: 512 << 20)
        configuration.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: configuration)
    }()

    func cached(_ url: URL) -> UIImage? {
        memory.object(forKey: url as NSURL)
    }

    func image(_ url: URL, authorization: String?) async -> UIImage? {
        if let image = cached(url) { return image }
        var request = URLRequest(url: url)
        request.setValue(authorization, forHTTPHeaderField: "Authorization")
        guard let (data, response) = try? await session.data(for: request),
              (response as? HTTPURLResponse)?.statusCode == 200,
              let image = UIImage(data: data)
        else { return nil }
        let decoded = await image.byPreparingForDisplay() ?? image
        memory.setObject(decoded, forKey: url as NSURL)
        return decoded
    }
}

/// A 2:3 poster, or the title on a plain card when there is none.
struct Poster: View {
    @Environment(AppModel.self) private var model
    let path: String?
    let title: String
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            Rectangle().fill(.quaternary)
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Text(title)
                    .font(.caption)
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .padding(8)
            }
        }
        .aspectRatio(2 / 3, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .task(id: path) {
            guard let url = model.absoluteURL(path) else {
                image = nil
                return
            }
            image = PosterCache.shared.cached(url)
            if image == nil {
                image = await PosterCache.shared.image(url, authorization: model.authorization(for: url))
            }
        }
    }
}

/// A focusable poster card: lifts under the Siri Remote focus on tvOS.
struct WorkCard: View {
    let work: Work

    var body: some View {
        NavigationLink(value: work) {
            VStack(alignment: .leading, spacing: 6) {
                Poster(path: work.posterUrl, title: work.title)
                #if os(iOS)
                Text(work.title).font(.caption).lineLimit(1)
                if let year = work.year {
                    Text(String(year)).font(.caption2).foregroundStyle(.secondary)
                }
                #endif
            }
        }
        #if os(tvOS)
        .buttonStyle(.card)
        #else
        .buttonStyle(.plain)
        #endif
        .accessibilityLabel(work.title)
    }
}

enum Metrics {
    #if os(tvOS)
    static let cardWidth: CGFloat = 220
    static let spacing: CGFloat = 48
    #else
    static let cardWidth: CGFloat = 120
    static let spacing: CGFloat = 12
    #endif
}
