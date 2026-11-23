import MetalKit
import SwiftUI

/// The globe's Metal surface and its gestures, as the web's renderer listens: drag to
/// pan (the ground follows the pointer); pinch or scroll to zoom; on the Mac the right
/// button, ⌥ or ⌃ with a drag to tilt and turn, a trackpad's rotate to turn, and the
/// arrow keys (Shift to tilt and turn) and + and − once the globe has focus; on the
/// iPhone two fingers twist to turn and move up or down to tilt. [FR-H-1]
struct GlobeSurface {
    let controller: GlobeController
    let outlines: GlobeOutlines
    let onFailed: () -> Void

    @MainActor final class Coordinator {
        var renderer: GlobeRenderer?
    }

    @MainActor func makeCoordinator() -> Coordinator { Coordinator() }

    @MainActor func make(_ coordinator: Coordinator) -> GlobeMTKView {
        let view = GlobeMTKView(frame: CGRect(x: 0, y: 0, width: 100, height: 100), device: MTLCreateSystemDefaultDevice())
        view.controller = controller
        guard view.device != nil, let renderer = GlobeRenderer(view: view, controller: controller, outlines: outlines) else {
            DispatchQueue.main.async { onFailed() }
            return view
        }
        coordinator.renderer = renderer
        view.delegate = renderer
        view.isPaused = true
        view.enableSetNeedsDisplay = true
        view.renderer = renderer
        controller.requestRender = { [weak view] in view?.requestDraw() }
        return view
    }
}

#if os(iOS)
import UIKit

extension GlobeSurface: UIViewRepresentable {
    func makeUIView(context: Context) -> GlobeMTKView { make(context.coordinator) }
    func updateUIView(_ view: GlobeMTKView, context: Context) {}
    static func dismantleUIView(_ view: GlobeMTKView, coordinator: Coordinator) {
        view.delegate = nil
        view.isPaused = true
    }
}

final class GlobeMTKView: MTKView, UIGestureRecognizerDelegate {
    weak var controller: GlobeController?
    var renderer: GlobeRenderer?
    private var lastPan = CGPoint.zero
    private var lastLift = CGPoint.zero

    override init(frame: CGRect, device: MTLDevice?) {
        super.init(frame: frame, device: device)
        isAccessibilityElement = false
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.maximumNumberOfTouches = 1
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        let rotate = UIRotationGestureRecognizer(target: self, action: #selector(onRotate(_:)))
        let lift = UIPanGestureRecognizer(target: self, action: #selector(onLift(_:)))
        lift.minimumNumberOfTouches = 2
        lift.maximumNumberOfTouches = 2
        for g in [pan, pinch, rotate, lift] as [UIGestureRecognizer] {
            g.delegate = self
            addGestureRecognizer(g)
        }
    }

    required init(coder: NSCoder) { fatalError("not used") }

    func requestDraw() { if isPaused { setNeedsDisplay() } }

    override func layoutSubviews() {
        super.layoutSubviews()
        controller?.setViewport(bounds.size)
    }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        true
    }

    @objc private func onPan(_ g: UIPanGestureRecognizer) {
        guard let c = controller else { return }
        let t = g.translation(in: self)
        switch g.state {
        case .began:
            c.beginGesture()
            lastPan = t
        case .changed:
            c.drag(dx: t.x - lastPan.x, dy: t.y - lastPan.y)
            lastPan = t
        default:
            c.endGesture()
        }
    }

    @objc private func onPinch(_ g: UIPinchGestureRecognizer) {
        guard let c = controller else { return }
        switch g.state {
        case .began: c.beginGesture()
        case .changed:
            if g.scale > 0 { c.pinch(zoom: 1 / Double(g.scale)) }
            g.scale = 1
        default: c.endGesture()
        }
    }

    @objc private func onRotate(_ g: UIRotationGestureRecognizer) {
        guard let c = controller else { return }
        switch g.state {
        case .began: c.beginGesture()
        case .changed:
            c.pinch(zoom: 1, twistDeg: Double(g.rotation) * 180 / .pi)
            g.rotation = 0
        default: c.endGesture()
        }
    }

    @objc private func onLift(_ g: UIPanGestureRecognizer) {
        guard let c = controller else { return }
        let t = g.translation(in: self)
        switch g.state {
        case .began:
            c.beginGesture()
            lastLift = t
        case .changed:
            c.pinch(zoom: 1, liftPx: -(t.y - lastLift.y))
            lastLift = t
        default: c.endGesture()
        }
    }
}
#else
import AppKit

extension GlobeSurface: NSViewRepresentable {
    func makeNSView(context: Context) -> GlobeMTKView { make(context.coordinator) }
    func updateNSView(_ view: GlobeMTKView, context: Context) {}
    static func dismantleNSView(_ view: GlobeMTKView, coordinator: Coordinator) {
        view.delegate = nil
        view.isPaused = true
    }
}

final class GlobeMTKView: MTKView {
    weak var controller: GlobeController?
    var renderer: GlobeRenderer?
    private var down: (point: CGPoint, moved: Bool, orbit: Bool)?
    /// The keyboard steps, as the web's: 60 points of pan, 0.7 of zoom, 5° of tilt, 10° of turn.
    static let keyStep = 60.0

    func requestDraw() { if isPaused { needsDisplay = true } }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func layout() {
        super.layout()
        controller?.setViewport(bounds.size)
    }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        controller?.setViewport(newSize)
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        controller?.beginGesture()
        let orbit = event.modifierFlags.contains(.control) || event.modifierFlags.contains(.option)
        down = (convert(event.locationInWindow, from: nil), false, orbit)
    }

    override func rightMouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        controller?.beginGesture()
        down = (convert(event.locationInWindow, from: nil), false, true)
    }

    override func mouseDragged(with event: NSEvent) { dragged(event) }
    override func rightMouseDragged(with event: NSEvent) { dragged(event) }

    private func dragged(_ event: NSEvent) {
        guard let c = controller, var d = down else { return }
        let p = convert(event.locationInWindow, from: nil)
        if !d.moved, abs(p.x - d.point.x) + abs(p.y - d.point.y) > 4 {
            d.moved = true
            down = d
        }
        guard d.moved else { return }
        // Event deltas run down the screen, as the web's pointer moves.
        if d.orbit { c.orbitDrag(dx: event.deltaX, dy: event.deltaY) } else { c.drag(dx: event.deltaX, dy: event.deltaY) }
    }

    override func mouseUp(with event: NSEvent) { ended() }
    override func rightMouseUp(with event: NSEvent) { ended() }

    private func ended() {
        if down?.moved == true { controller?.endGesture() }
        down = nil
    }

    override func scrollWheel(with event: NSEvent) {
        let unit = event.hasPreciseScrollingDeltas ? 1.0 : 16.0
        // A browser's wheel runs the other way from AppKit's scrolling delta.
        controller?.wheel(deltaY: -Double(event.scrollingDeltaY) * unit)
    }

    override func magnify(with event: NSEvent) {
        controller?.pinch(zoom: 1 / max(0.05, 1 + Double(event.magnification)))
        if event.phase == .ended { controller?.endGesture() }
    }

    override func rotate(with event: NSEvent) {
        controller?.pinch(zoom: 1, twistDeg: -Double(event.rotation))
        if event.phase == .ended { controller?.endGesture() }
    }

    override func keyDown(with event: NSEvent) {
        guard let c = controller else { return super.keyDown(with: event) }
        let shift = event.modifierFlags.contains(.shift)
        let step = Self.keyStep
        switch event.keyCode {
        case 123: shift ? c.orbit(tilt: 0, turn: -10) : c.nudge(dx: step, dy: 0)
        case 124: shift ? c.orbit(tilt: 0, turn: 10) : c.nudge(dx: -step, dy: 0)
        case 126: shift ? c.orbit(tilt: 5) : c.nudge(dx: 0, dy: step)
        case 125: shift ? c.orbit(tilt: -5) : c.nudge(dx: 0, dy: -step)
        default:
            switch event.charactersIgnoringModifiers {
            case "+", "=": c.nudge(dx: 0, dy: 0, zoom: 0.7)
            case "-": c.nudge(dx: 0, dy: 0, zoom: 1 / 0.7)
            default: super.keyDown(with: event)
            }
        }
    }
}
#endif
