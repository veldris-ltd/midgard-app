import SwiftUI

/// The MAP view's drawing surface and gestures, as the web's map listens: drag to pan,
/// pinch or scroll to zoom about the pointer, double-click or double-tap to zoom in, and
/// on the Mac the arrow keys and + and − once the map has focus. Drawn with CoreGraphics
/// on demand, and each frame while a flight eases. [FR-H-1]
struct MapSurface {
    let controller: MapController
}

#if os(iOS)
import UIKit

extension MapSurface: UIViewRepresentable {
    func makeUIView(context: Context) -> MapDrawView {
        let v = MapDrawView(controller: controller)
        controller.requestRender = { [weak v] in v?.setNeedsDisplay() }
        return v
    }

    func updateUIView(_ view: MapDrawView, context: Context) {}

    static func dismantleUIView(_ view: MapDrawView, coordinator: ()) { view.stopTimer() }
}

final class MapDrawView: UIView {
    let controller: MapController
    private var timer: Timer?
    private var lastPan = CGPoint.zero

    init(controller: MapController) {
        self.controller = controller
        super.init(frame: .zero)
        isOpaque = true
        contentMode = .redraw
        isAccessibilityElement = false
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.maximumNumberOfTouches = 1
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        let tap = UITapGestureRecognizer(target: self, action: #selector(onDoubleTap(_:)))
        tap.numberOfTapsRequired = 2
        for g in [pan, pinch, tap] as [UIGestureRecognizer] { addGestureRecognizer(g) }
    }

    required init?(coder: NSCoder) { nil }

    func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        controller.setViewport(bounds.size)
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        guard let g = UIGraphicsGetCurrentContext() else { return }
        let more = controller.tick(now: CACurrentMediaTime())
        controller.draw(g)
        if more, timer == nil {
            timer = Timer.scheduledTimer(withTimeInterval: 1 / 60, repeats: true) { [weak self] _ in self?.setNeedsDisplay() }
        } else if !more {
            stopTimer()
        }
    }

    @objc private func onPan(_ g: UIPanGestureRecognizer) {
        let t = g.translation(in: self)
        switch g.state {
        case .began:
            controller.beginGesture()
            lastPan = t
        case .changed:
            controller.drag(dx: t.x - lastPan.x, dy: t.y - lastPan.y)
            lastPan = t
        default:
            controller.endGesture()
        }
    }

    @objc private func onPinch(_ g: UIPinchGestureRecognizer) {
        switch g.state {
        case .began: controller.beginGesture()
        case .changed:
            if g.scale > 0 { controller.zoom(by: log2(Double(g.scale)), at: g.location(in: self)) }
            g.scale = 1
        default: controller.endGesture()
        }
    }

    @objc private func onDoubleTap(_ g: UITapGestureRecognizer) {
        controller.zoom(by: 1, at: g.location(in: self))
        controller.endGesture()
    }
}
#else
import AppKit

extension MapSurface: NSViewRepresentable {
    func makeNSView(context: Context) -> MapDrawView {
        let v = MapDrawView(controller: controller)
        controller.requestRender = { [weak v] in v?.needsDisplay = true }
        return v
    }

    func updateNSView(_ view: MapDrawView, context: Context) {}

    static func dismantleNSView(_ view: MapDrawView, coordinator: ()) { view.stopTimer() }
}

final class MapDrawView: NSView {
    let controller: MapController
    private var timer: Timer?
    private var downAt: CGPoint?
    /// The keyboard step, as the web's: 80 points of pan, half a zoom level.
    static let keyStep = 80.0

    init(controller: MapController) {
        self.controller = controller
        super.init(frame: .zero)
        wantsLayer = true
        layerContentsRedrawPolicy = .onSetNeedsDisplay
    }

    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override var isOpaque: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        controller.setViewport(newSize)
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let g = NSGraphicsContext.current?.cgContext else { return }
        let more = controller.tick(now: CACurrentMediaTime())
        controller.draw(g)
        if more, timer == nil {
            timer = Timer.scheduledTimer(withTimeInterval: 1 / 60, repeats: true) { [weak self] _ in self?.needsDisplay = true }
        } else if !more {
            stopTimer()
        }
    }

    private func local(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        controller.beginGesture()
        downAt = local(event)
        if event.clickCount == 2 {
            controller.zoom(by: 1, at: local(event))
            controller.endGesture()
        }
    }

    override func mouseDragged(with event: NSEvent) {
        guard downAt != nil, event.deltaX != 0 || event.deltaY != 0 else { return }
        controller.drag(dx: event.deltaX, dy: event.deltaY)
    }

    override func mouseUp(with event: NSEvent) {
        downAt = nil
        controller.endGesture()
    }

    override func scrollWheel(with event: NSEvent) {
        let unit = event.hasPreciseScrollingDeltas ? 1.0 : 16.0
        controller.zoom(by: Double(event.scrollingDeltaY) * unit * 0.002, at: local(event))
    }

    override func magnify(with event: NSEvent) {
        controller.zoom(by: log2(max(0.05, 1 + Double(event.magnification))), at: local(event))
    }

    override func keyDown(with event: NSEvent) {
        let s = Self.keyStep
        switch event.keyCode {
        case 123: controller.nudge(dx: s, dy: 0)
        case 124: controller.nudge(dx: -s, dy: 0)
        case 126: controller.nudge(dx: 0, dy: s)
        case 125: controller.nudge(dx: 0, dy: -s)
        default:
            switch event.charactersIgnoringModifiers {
            case "+", "=": controller.nudge(dx: 0, dy: 0, dZoom: 0.5)
            case "-": controller.nudge(dx: 0, dy: 0, dZoom: -0.5)
            default: super.keyDown(with: event)
            }
        }
    }
}
#endif
