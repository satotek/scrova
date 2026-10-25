import ScrovaKit
import SwiftUI

/// The track's timeline with a mark where it will scrobble.
///
/// The mark is placed from the engine's listening time, not from a fixed
/// fraction of the track: seeks and pauses are not listening, so after a
/// forward seek the mark moves ahead with the playhead.
struct ScrobbleMeter: View {
    let progress: SessionProgress
    var tint: Color?

    var body: some View {
        // Observers report every few seconds; extrapolating between readings
        // keeps the playhead moving at the speed of the music.
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let reading = Reading(progress: progress, at: context.date)
            VStack(alignment: .leading, spacing: 6) {
                if let duration = progress.track.duration, duration > 0 {
                    timeline(reading, duration: duration)
                }
                labels(reading)
                    .font(.caption)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
        .accessibilityElement(children: .combine)
    }

    private var accent: Color { tint ?? .accentColor }

    private func timeline(_ reading: Reading, duration: TimeInterval) -> some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            let x = { (time: TimeInterval) in width * min(max(time / duration, 0), 1) }
            ZStack(alignment: .leading) {
                Capsule().fill(.quaternary)
                    .frame(height: 4)
                // The played part takes the accent only once it has counted, so
                // the moment of scrobbling is visible on the bar itself.
                Capsule()
                    .fill(progress.scrobbled ? AnyShapeStyle(accent) : AnyShapeStyle(.secondary))
                    .frame(width: x(reading.position ?? 0), height: 4)
                if !progress.scrobbled, let mark = reading.scrobblePosition, mark <= duration {
                    RoundedRectangle(cornerRadius: 1)
                        .fill(accent)
                        .frame(width: 3, height: 12)
                        .offset(x: x(mark) - 1.5)
                }
            }
            .frame(maxHeight: .infinity)
        }
        .frame(height: 12)
    }

    private func labels(_ reading: Reading) -> some View {
        HStack {
            Text(Self.clock(reading.position ?? 0))
            Spacer()
            status(reading)
            Spacer()
            Text(progress.track.duration.map(Self.clock) ?? "")
        }
    }

    @ViewBuilder
    private func status(_ reading: Reading) -> some View {
        if progress.scrobbled {
            HStack(spacing: 3) {
                Image(systemName: "checkmark")
                Text("Scrobbled")
            }
            .foregroundStyle(accent)
        } else if let remaining = reading.remaining {
            Text("Scrobbles in \(Self.clock(remaining))")
        } else if progress.track.duration.map({ $0 <= 30 }) ?? false {
            Text("Too short to scrobble")
        } else {
            Text("Can’t scrobble without a title and artist")
        }
    }

    static func clock(_ interval: TimeInterval) -> String {
        let seconds = max(0, Int(interval.rounded(.down)))
        return String(format: "%d:%02d", seconds / 60, seconds % 60)
    }
}

/// The engine's last reading carried forward to `date`.
private struct Reading {
    var position: TimeInterval?
    /// Listening time still needed; `nil` when the track will not scrobble.
    var remaining: TimeInterval?
    /// Where the playhead will be when it scrobbles, if playback continues.
    var scrobblePosition: TimeInterval?

    init(progress: SessionProgress, at date: Date) {
        let elapsed = progress.state == .playing ? max(0, date.timeIntervalSince(progress.observedAt)) : 0
        let duration = progress.track.duration ?? .infinity
        position = progress.position.map { min($0 + elapsed, duration) }
        if let threshold = progress.threshold, !progress.scrobbled {
            let remaining = max(0, threshold - progress.played - elapsed)
            self.remaining = remaining
            scrobblePosition = position.map { $0 + remaining }
        }
    }
}
