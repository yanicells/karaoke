import SwiftUI

package struct ContentView: View {
    @Environment(MicController.self) var controller

    package init() {}

    package var body: some View {
        @Bindable var controller = controller

        VStack(alignment: .leading, spacing: 20) {
            // Input device picker
            VStack(alignment: .leading, spacing: 4) {
                Label("Microphone", systemImage: "mic")
                    .font(.headline)
                Picker("Microphone", selection: $controller.selectedInputDevice) {
                    Text("None").tag(Optional<AudioDevice>.none)
                    ForEach(controller.inputDevices) { device in
                        Text(device.name).tag(Optional(device))
                    }
                }
                .labelsHidden()
            }

            // Output device picker
            VStack(alignment: .leading, spacing: 4) {
                Label("Speaker", systemImage: "speaker.wave.2")
                    .font(.headline)
                Picker("Speaker", selection: $controller.selectedOutputDevice) {
                    Text("None").tag(Optional<AudioDevice>.none)
                    ForEach(controller.outputDevices) { device in
                        HStack {
                            Text(device.name)
                            if device.isBluetooth {
                                Image(systemName: "b.circle")
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .tag(Optional(device))
                    }
                }
                .labelsHidden()
            }

            // Gain slider
            VStack(alignment: .leading, spacing: 4) {
                Label("Mic Volume", systemImage: "slider.horizontal.3")
                    .font(.headline)
                Slider(value: $controller.gain, in: 0...2, step: 0.05)
            }

            Divider()

            // Start/Stop + Mute
            HStack(spacing: 12) {
                Button {
                    Task { await controller.toggleRunning() }
                } label: {
                    Label(
                        controller.isRunning ? "Stop" : "Start",
                        systemImage: controller.isRunning ? "stop.circle.fill" : "play.circle.fill"
                    )
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(controller.isRunning ? .red : .accentColor)

                Button {
                    controller.toggleMute()
                } label: {
                    Label(
                        controller.isMuted ? "Unmute" : "Mute",
                        systemImage: controller.isMuted ? "mic.slash.fill" : "mic.fill"
                    )
                }
                .buttonStyle(.bordered)
                .disabled(!controller.isRunning)
            }

            // Latency readout + Bluetooth warning
            if controller.isRunning {
                latencyView
            }

            // Error message
            if let message = controller.errorMessage {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(20)
        .frame(width: 360)
        .onAppear { controller.refreshDevices() }
    }

    @ViewBuilder
    private var latencyView: some View {
        let ms = Int(controller.estimatedLatencyMs)
        let bluetoothOutput = controller.selectedOutputDevice?.isBluetooth == true

        HStack(spacing: 6) {
            if bluetoothOutput {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.yellow)
                Text("Bluetooth output — latency will be high (\(ms) ms)")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "waveform.circle.fill")
                    .foregroundStyle(.green)
                Text("\(ms) ms round-trip")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
