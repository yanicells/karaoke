import SwiftUI
import KaraokeMicCore

@main
struct KaraokeMicApp: App {
    @State private var controller = MicController()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(controller)
        }
        .windowResizability(.contentSize)
        .defaultSize(width: 360, height: 400)
    }
}
