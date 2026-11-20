import SwiftUI
#if canImport(UIKit)
import UIKit
import UIKit.UIGestureRecognizerSubclass
#endif

/// Calls `onTouch` whenever a finger touches down anywhere in the window it is placed
/// in, to postpone the idle lock, without being a gesture SwiftUI can see: a
/// `TapGesture` over the signed-in screen made SwiftUI report every element under it
/// (the status chips) as tappable, and the hit-region audit then failed them as too
/// small. Here a gesture recognizer on the window notices the touch, never recognises
/// and never delays or cancels a touch, so taps and scrolls work as without it; the
/// view itself is empty and hidden from accessibility. It sees touches only: typing on
/// a hardware keyboard, key commands and VoiceOver moving its focus are not touches,
/// and the iOS app postpones the lock for those itself. On macOS it does nothing: the
/// app watches mouse and key events with an `NSEvent` monitor. [FR-C-9] [RC-49]
public struct IdleTouchWatcher: View {
    let onTouch: @MainActor () -> Void
    public init(_ onTouch: @escaping @MainActor () -> Void) { self.onTouch = onTouch }

    public var body: some View {
        #if canImport(UIKit)
        WindowTouchInstaller(onTouch: onTouch).accessibilityHidden(true)
        #else
        Color.clear.frame(width: 0, height: 0).accessibilityHidden(true)
        #endif
    }
}

#if canImport(UIKit)
/// Installs one `TouchNoticer` on its view's window while the view is in it.
struct WindowTouchInstaller: UIViewRepresentable {
    let onTouch: @MainActor () -> Void

    func makeUIView(context: Context) -> InstallerView {
        let view = InstallerView()
        view.isUserInteractionEnabled = false
        view.isAccessibilityElement = false
        view.noticer.onTouch = onTouch
        return view
    }

    func updateUIView(_ view: InstallerView, context: Context) {
        view.noticer.onTouch = onTouch
    }

    static func dismantleUIView(_ view: InstallerView, coordinator: ()) {
        view.noticer.view?.removeGestureRecognizer(view.noticer)
    }

    final class InstallerView: UIView {
        let noticer = TouchNoticer()

        override func didMoveToWindow() {
            super.didMoveToWindow()
            if noticer.view !== window {
                noticer.view?.removeGestureRecognizer(noticer)
                window?.addGestureRecognizer(noticer)
            }
        }
    }
}

/// A recogniser that reports each touch that begins and then fails at once: it never
/// recognises, so it takes no touch from any other recogniser or view.
final class TouchNoticer: UIGestureRecognizer, UIGestureRecognizerDelegate {
    var onTouch: @MainActor () -> Void = {}

    init() {
        super.init(target: nil, action: nil)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
        delegate = self
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        // UIGestureRecognizer is main-actor isolated: so is this override.
        onTouch()
        state = .failed
    }

    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { false }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
                           shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
}
#endif
