import CoreGraphics
import SwiftUI

/// Picks an accent colour from album artwork, so the scrobble meter belongs to
/// the track that is playing instead of to the app.
enum ArtworkTint {
    /// A plain average turns most artwork into mud, so this samples a tiny
    /// thumbnail and keeps the most vivid pixel.
    static func color(for image: CGImage) -> Color? {
        let side = 12
        var pixels = [UInt8](repeating: 0, count: side * side * 4)
        let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let context = CGContext(
                data: buffer.baseAddress, width: side, height: side, bitsPerComponent: 8,
                bytesPerRow: side * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.interpolationQuality = .medium
            context.draw(image, in: CGRect(x: 0, y: 0, width: side, height: side))
            return true
        }
        guard drawn else { return nil }

        var best: (hue: Double, saturation: Double, score: Double)?
        for offset in stride(from: 0, to: pixels.count, by: 4) {
            let (h, s, b) = hsb(
                Double(pixels[offset]) / 255, Double(pixels[offset + 1]) / 255, Double(pixels[offset + 2]) / 255
            )
            // Near-black pixels have unstable hues.
            guard b > 0.25 else { continue }
            let score = s * b
            if score > (best?.score ?? 0) { best = (h, s, score) }
        }
        // Greyscale artwork has no colour worth borrowing.
        guard let best, best.saturation > 0.2 else { return nil }
        // Fixed brightness keeps the meter legible on light and dark materials.
        return Color(hue: best.hue, saturation: min(best.saturation, 0.7), brightness: 0.85)
    }

    private static func hsb(_ r: Double, _ g: Double, _ b: Double) -> (Double, Double, Double) {
        let maxValue = max(r, g, b)
        let delta = maxValue - min(r, g, b)
        guard delta > 0 else { return (0, 0, maxValue) }
        var hue: Double = switch maxValue {
        case r: (g - b) / delta
        case g: (b - r) / delta + 2
        default: (r - g) / delta + 4
        }
        hue /= 6
        if hue < 0 { hue += 1 }
        return (hue, delta / maxValue, maxValue)
    }
}
