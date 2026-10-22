// Placeholder entry point; the SwiftUI app replaces it in a later task (the window).
import MidgardShellCore

let paths = Paths.current()
if let problem = SystemCheck.problemOnThisMac() {
    print(problem)
} else {
    print("Midgard shell: resources \(paths.resources.path)")
}
