import CryptoKit
import Foundation
import ImageIO
import ScrovaKit
import UniformTypeIdentifiers

/// Small on-disk thumbnails of artwork seen while a track played, so the
/// scrobble history can show covers after the player has moved on. Neither
/// Music nor the Rust queue keeps artwork per scrobble.
///
/// Lives in Caches: losing it only costs the covers of older entries.
enum ArtworkCache {
    /// Twice the largest history thumbnail at 3x.
    private static let maxPixelSize = 240

    private static let directory: URL? = {
        guard let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first else {
            return nil
        }
        let directory = caches.appending(path: "Artwork", directoryHint: .isDirectory)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }()

    static func store(_ image: CGImage, for track: Track) {
        guard let url = url(for: track),
              !FileManager.default.fileExists(atPath: url.path(percentEncoded: false)),
              let thumbnail = thumbnail(of: image),
              let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.jpeg.identifier as CFString, 1, nil)
        else { return }
        CGImageDestinationAddImage(destination, thumbnail, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
        CGImageDestinationFinalize(destination)
    }

    static func image(for track: Track) -> CGImage? {
        guard let url = url(for: track),
              let source = CGImageSourceCreateWithURL(url as CFURL, nil)
        else { return nil }
        return CGImageSourceCreateImageAtIndex(source, 0, nil)
    }

    private static func thumbnail(of image: CGImage) -> CGImage? {
        let scale = min(1, Double(maxPixelSize) / Double(max(image.width, image.height)))
        let width = max(1, Int(Double(image.width) * scale))
        let height = max(1, Int(Double(image.height) * scale))
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue
        ) else { return nil }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        return context.makeImage()
    }

    /// Keyed per album where possible: tracks of one album share a cover, and
    /// the album name survives into the queue while artwork does not.
    private static func url(for track: Track) -> URL? {
        let artist = track.albumArtist ?? track.artist
        let key = track.album.map { "album|\(artist)|\($0)" } ?? "track|\(track.artist)|\(track.title)"
        let digest = SHA256.hash(data: Data(key.lowercased().utf8))
        let name = digest.prefix(16).map { String(format: "%02x", $0) }.joined()
        return directory?.appending(path: "\(name).jpg")
    }
}
