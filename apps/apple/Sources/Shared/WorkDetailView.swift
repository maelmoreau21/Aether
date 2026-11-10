import SwiftUI

/// A film or a series: poster, synopsis, play or resume, offline copy (iOS),
/// and for a series its seasons and episodes.
struct WorkDetailView: View {
    @Environment(AppModel.self) private var model
    let work: Work
    @State private var detail: WorkDetail?
    @State private var progress: Progress?
    @State private var season = 0

    private var isSeries: Bool { work.kind == "series" }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                if let seasons = detail?.seasons, !seasons.isEmpty {
                    seasonsView(seasons)
                }
            }
            .padding()
        }
        .navigationTitle(work.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .task(id: model.generation) { await load() }
    }

    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 32) {
                Poster(path: work.posterUrl, title: work.title).frame(width: posterWidth)
                summary
            }
            VStack(alignment: .leading, spacing: 16) {
                Poster(path: work.posterUrl, title: work.title).frame(width: posterWidth)
                summary
            }
        }
    }

    private var posterWidth: CGFloat {
        #if os(tvOS)
        360
        #else
        180
        #endif
    }

    private var summary: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(work.title).font(.largeTitle.bold())
            if let year = work.year {
                Text(String(year)).foregroundStyle(.secondary)
            }
            if !isSeries {
                HStack(spacing: 16) {
                    Button {
                        model.play(workId: work.id, title: work.title)
                    } label: {
                        Label(resumeLabel, systemImage: "play.fill")
                    }
                    .buttonStyle(.borderedProminent)
                    #if os(iOS)
                    DownloadButton(workId: work.id, title: work.title)
                    #endif
                }
                if let progress, progress.position > 0, !progress.watched {
                    ProgressView(value: progress.position, total: max(progress.duration, 1))
                        .frame(maxWidth: 320)
                }
            }
            let description = detail?.work.description ?? work.description
            if !description.isEmpty {
                Text(description)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    #if os(tvOS)
                    .focusable() // lets the remote scroll a long synopsis
                    #endif
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var resumeLabel: String {
        guard let progress, progress.position > 30, !progress.watched else { return "Lire" }
        return "Reprendre à \(clock(progress.position))"
    }

    @ViewBuilder
    private func seasonsView(_ seasons: [Season]) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            if seasons.count > 1 {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 12) {
                        ForEach(Array(seasons.enumerated()), id: \.element.id) { index, item in
                            Button(seasonName(item)) { season = index }
                                .buttonStyle(.bordered)
                                .tint(index == season ? Color.accentColor : Color.secondary)
                        }
                    }
                    #if os(tvOS)
                    .padding(.vertical, 12)
                    #endif
                }
            } else {
                Text(seasonName(seasons[0])).font(.title3.bold())
            }
            let current = seasons[min(season, seasons.count - 1)]
            LazyVStack(alignment: .leading, spacing: 8) {
                ForEach(current.episodes, id: \.id) { episode in
                    EpisodeRow(episode: episode, series: work.title)
                }
            }
        }
    }

    private func seasonName(_ season: Season) -> String {
        if !season.title.isEmpty { return season.title }
        if let number = season.number { return "Saison \(number)" }
        return "Épisodes"
    }

    private func load() async {
        guard let client = model.client else { return }
        do {
            detail = try await client.work(id: work.id)
            if !isSeries { progress = try await client.progress(workId: work.id) }
        } catch {
            model.fail(error)
        }
    }
}

struct EpisodeRow: View {
    @Environment(AppModel.self) private var model
    let episode: Episode
    let series: String

    var body: some View {
        HStack(spacing: 12) {
            Button {
                model.play(workId: episode.id, title: title)
            } label: {
                HStack(alignment: .top, spacing: 16) {
                    Image(systemName: "play.circle.fill").font(.title2)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(heading).font(.headline)
                        if !episode.description.isEmpty {
                            Text(episode.description)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                                .lineLimit(3)
                        }
                    }
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            #if os(iOS)
            DownloadButton(workId: episode.id, title: title)
            #endif
        }
        .padding(.vertical, 6)
    }

    private var heading: String {
        guard let number = episode.number else { return episode.title }
        return "\(number). \(episode.title)"
    }

    private var title: String { "\(series) – \(heading)" }
}

/// `1:02:03` or `12:34`.
func clock(_ seconds: Double) -> String {
    let total = Int(max(0, seconds))
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}
