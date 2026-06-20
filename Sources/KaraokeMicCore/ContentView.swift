import SwiftUI

package struct ContentView: View {
    @Environment(MicController.self) var controller

    package init() {}

    package var body: some View {
        Text("KaraokeMic — coming soon")
            .padding()
            .frame(width: 360)
    }
}
