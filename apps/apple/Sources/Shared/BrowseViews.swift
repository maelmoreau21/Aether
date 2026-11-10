import SwiftUI

extension View {
    /// The pages every tab's navigation stack can push.
    func aetherDestinations() -> some View {
        navigationDestination(for: Work.self) { WorkDetailView(work: $0) }
            .navigationDestination(for: MediaLibrary.self) { LibraryView(library: $0) }
    }
}

/// A titled horizontal row of cards.
struct Shelf<Content: View>: View {
    let title: String
    var library: MediaLibrary?
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let library {
                NavigationLink(value: library) {
                    HStack(spacing: 6) {
                        Text(title)
                        Image(systemName: "chevron.right").font(.footnote)
                    }
                    .font(.title3.bold())
                }
                .buttonStyle(.plain)
            } else {
                Text(title).font(.title3.bold())
            }
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: Metrics.spacing) { content }
                    #if os(tvOS)
                    .padding(.vertical, 24)
                    #endif
            }
            #if os(tvOS)
            .scrollClipDisabled()
            #endif
        }
        .padding(.horizontal)
    }
}

// MARK: Home

struct HomeView: View {
    @Environment(AppModel.self) private var model
    @State private var resume: [ResumePoint] = []
    @State private var posters: [String: String] = [:]
    @State private var shelves: [LibraryRow] = []
    @State private var loaded = false

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 32) {
                if !resume.isEmpty {
                    Shelf(title: "Continuer") {
                        ForEach(resume, id: \.workId) { point in
                            ResumeCard(point: point, poster: posters[point.workId])
                        }
                    }
                }
                ForEach(shelves) { shelf in
                    Shelf(title: shelf.library.name, library: shelf.library) {
                        ForEach(shelf.works, id: \.id) { WorkCard(work: $0).frame(width: Metrics.cardWidth) }
                    }
                }
                if loaded, resume.isEmpty, shelves.isEmpty {
                    ContentUnavailableView("Aucun titre", systemImage: "film.stack")
                }
            }
            .padding(.vertical)
        }
        .navigationTitle("Accueil")
        .aetherDestinations()
        .task(id: model.generation) { await load() }
        .refreshable { await load() }
    }

    private func load() async {
        guard let client = model.client else { return }
        do {
            async let points = client.continueWatching()
            async let libraries = client.libraries()
            resume = Array((try await points).prefix(30))
            var rows: [LibraryRow] = []
            for library in try await libraries {
                let page = try await client.libraryCatalog(library: library.id, search: nil, offset: 0, limit: 20)
                if !page.items.isEmpty { rows.append(LibraryRow(library: library, works: page.items)) }
            }
            shelves = rows
        } catch {
            model.fail(error)
        }
        loaded = true
        // An episode shows its series' poster.
        for point in resume where posters[point.workId] == nil {
            if let detail = try? await client.work(id: point.seriesId ?? point.workId) {
                posters[point.workId] = detail.work.posterUrl
            }
        }
    }
}

struct LibraryRow: Identifiable {
    let library: MediaLibrary
    let works: [Work]
    var id: String { library.id }
}

struct ResumeCard: View {
    @Environment(AppModel.self) private var model
    let point: ResumePoint
    let poster: String?

    var body: some View {
        Button {
            model.play(workId: point.workId, title: point.title)
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                ZStack(alignment: .bottom) {
                    Poster(path: poster, title: point.title)
                    ProgressView(value: point.position, total: max(point.duration, 1))
                        .tint(.accentColor)
                        .padding(6)
                }
                Text(point.title).font(.caption).lineLimit(1)
                Text(remaining).font(.caption2).foregroundStyle(.secondary)
            }
            .frame(width: Metrics.cardWidth)
        }
        #if os(tvOS)
        .buttonStyle(.card)
        #else
        .buttonStyle(.plain)
        #endif
    }

    private var remaining: String {
        let minutes = Int((point.duration - point.position) / 60)
        return minutes < 1 ? "Bientôt fini" : "Encore \(minutes) min"
    }
}

// MARK: Libraries

struct LibrariesView: View {
    @Environment(AppModel.self) private var model
    @State private var libraries: [MediaLibrary] = []

    var body: some View {
        List(libraries, id: \.id) { library in
            NavigationLink(value: library) {
                Label(library.name, systemImage: icon(library.kind))
            }
        }
        .navigationTitle("Bibliothèques")
        .aetherDestinations()
        .task(id: model.generation) {
            guard let client = model.client else { return }
            do { libraries = try await client.libraries() } catch { model.fail(error) }
        }
    }

    private func icon(_ kind: String) -> String {
        switch kind {
        case "series", "shows", "tv": return "tv"
        case "music": return "music.note"
        default: return "film"
        }
    }
}

/// One library as a grid, a page at a time.
struct LibraryView: View {
    @Environment(AppModel.self) private var model
    let library: MediaLibrary
    @State private var works: [Work] = []
    @State private var hasMore = true
    @State private var loading = false

    var body: some View {
        WorkGrid(works: works) { work in
            if work.id == works.last?.id { Task { await loadMore() } }
        }
        .navigationTitle(library.name)
        .task { if works.isEmpty { await loadMore() } }
    }

    private func loadMore() async {
        guard hasMore, !loading, let client = model.client else { return }
        loading = true
        defer { loading = false }
        do {
            let page = try await client.libraryCatalog(
                library: library.id, search: nil, offset: UInt32(works.count), limit: 60)
            works += page.items
            hasMore = page.hasMore
        } catch {
            hasMore = false
            model.fail(error)
        }
    }
}

struct WorkGrid: View {
    let works: [Work]
    var onAppear: (Work) -> Void = { _ in }

    var body: some View {
        ScrollView {
            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: Metrics.cardWidth), spacing: Metrics.spacing)],
                spacing: Metrics.spacing
            ) {
                ForEach(works, id: \.id) { work in
                    WorkCard(work: work).onAppear { onAppear(work) }
                }
            }
            .padding()
        }
    }
}

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var query = ""
    @State private var works: [Work] = []

    var body: some View {
        WorkGrid(works: works)
            .overlay {
                if !query.isEmpty, works.isEmpty { ContentUnavailableView.search(text: query) }
            }
            .navigationTitle("Rechercher")
            .searchable(text: $query, prompt: "Films, séries…")
            .aetherDestinations()
            .task(id: query) {
                let text = query.trimmingCharacters(in: .whitespaces)
                guard !text.isEmpty, let client = model.client else {
                    works = []
                    return
                }
                try? await Task.sleep(for: .milliseconds(300))
                guard !Task.isCancelled else { return }
                do {
                    works = try await client.catalog(kind: nil, search: text, offset: 0, limit: 60).items
                } catch {
                    model.fail(error)
                }
            }
    }
}
