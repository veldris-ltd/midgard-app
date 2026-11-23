import MidgardKit
import SwiftUI

/// The user's own layers (Your Data) on MAP and 3D, at their true positions: an open
/// diamond in the warning tone for each observation of an UNVERIFIED FEED, a small
/// square in the accent for each imported object, and a legend naming each layer as it
/// is labelled everywhere. Drawn under the tracks and never fused with them; not
/// interactive (the layer list names each layer). [FR-H-1]
struct UserLayerPoints: View {
    let layers: [UserMapLayer]
    /// Changes as the camera moves, so the points are drawn again.
    let tick: Int
    let project: (Double, Double) -> CGPoint?

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            Canvas { ctx, size in
                _ = tick
                for layer in layers {
                    for p in layer.points {
                        guard let c = project(p.lat, p.lon), c.x > -10, c.y > -10, c.x < size.width + 10,
                              c.y < size.height + 10 else { continue }
                        if layer.unverified {
                            var d = Path()
                            d.move(to: CGPoint(x: c.x, y: c.y - 5))
                            d.addLine(to: CGPoint(x: c.x + 5, y: c.y))
                            d.addLine(to: CGPoint(x: c.x, y: c.y + 5))
                            d.addLine(to: CGPoint(x: c.x - 5, y: c.y))
                            d.closeSubpath()
                            ctx.stroke(d, with: .color(MGWeb.warn), lineWidth: 1.5)
                        } else {
                            ctx.fill(Path(CGRect(x: c.x - 3.5, y: c.y - 3.5, width: 7, height: 7)),
                                     with: .color(MGWebAccent.heimdall.fill))
                        }
                    }
                }
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            if !layers.isEmpty {
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(layers) { layer in
                        HStack(spacing: 6) {
                            Image(systemName: layer.unverified ? "diamond" : "square.fill")
                                .font(.system(size: 9))
                                .foregroundStyle(layer.unverified ? MGWeb.warn : MGWebAccent.heimdall.fill)
                                .accessibilityHidden(true)
                            Text("\(layer.label) · \(layer.points.count)")
                                .font(MGWebFont.mono(10))
                                .foregroundStyle(MGWeb.textBody)
                                .lineLimit(1)
                        }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .background(RoundedRectangle(cornerRadius: 4).fill(MGWeb.surfaceCard.opacity(0.92)))
                .padding(.leading, 12)
                .padding(.bottom, 64)
                .allowsHitTesting(false)
                .accessibilityElement(children: .combine)
                .accessibilityLabel(Text("Your layers: \(layers.map { "\($0.label), \($0.points.count) placed" }.joined(separator: "; "))"))
                .accessibilityIdentifier("mg.heimdall.userlayers.legend")
            }
        }
    }
}
