use crate::audio::{AudioDevice, Devices};
use crate::worker::{Command, DEVICE_LOST, Event};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Stopped,
    Starting,
    Running,
    Stopping,
}

/// UI state only. Native audio work is expressed as commands to the worker.
pub struct Controller {
    pub phase: Phase,
    pub devices: Devices,
    pub input: Option<u32>,
    pub output: Option<u32>,
    pub gain: f32,
    pub muted: bool,
    pub latency_ms: f64,
    pub error: Option<String>,
    last_devices_error: Option<String>,
    request_id: u64,
    auto_input: bool,
    auto_output: bool,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            phase: Phase::Stopped,
            devices: Devices {
                inputs: Vec::new(),
                outputs: Vec::new(),
            },
            input: None,
            output: None,
            gain: 1.0,
            muted: false,
            latency_ms: 0.0,
            error: None,
            last_devices_error: None,
            request_id: 0,
            auto_input: true,
            auto_output: true,
        }
    }
}

impl Controller {
    pub fn toggle_running(&mut self) -> Option<Command> {
        match self.phase {
            Phase::Stopped => self.start(),
            Phase::Running => Some(self.stop()),
            Phase::Starting | Phase::Stopping => None,
        }
    }

    pub fn toggle_mute(&mut self) -> Option<Command> {
        self.muted = !self.muted;
        self.is_active().then_some(Command::SetMuted(self.muted))
    }

    pub fn set_gain(&mut self, gain: f32) -> Option<Command> {
        if !gain.is_finite() {
            return None;
        }
        let gain = gain.clamp(0.0, 2.0);
        if self.gain == gain {
            return None;
        }
        self.gain = gain;
        self.is_active().then_some(Command::SetGain(gain))
    }

    pub fn select_input(&mut self, input: Option<u32>) -> Option<Command> {
        self.auto_input = false;
        let valid = input.filter(|id| contains(&self.devices.inputs, *id));
        if input != valid {
            self.error = Some("The selected microphone is no longer available.".into());
        }
        if self.input == valid {
            return None;
        }
        self.input = valid;
        if input == valid {
            self.error = None;
        }
        self.route_changed()
    }

    pub fn select_output(&mut self, output: Option<u32>) -> Option<Command> {
        self.auto_output = false;
        let valid = output.filter(|id| contains(&self.devices.outputs, *id));
        if output != valid {
            self.error = Some("The selected speaker is no longer available.".into());
        }
        if self.output == valid {
            return None;
        }
        self.output = valid;
        if output == valid {
            self.error = None;
        }
        self.route_changed()
    }

    /// Returned commands must be sent just like commands from UI actions.
    pub fn apply(&mut self, event: Event) -> Option<Command> {
        match event {
            Event::Devices(devices) => self.refresh_devices(devices),
            Event::DevicesError(error) => {
                self.last_devices_error = Some(error.clone());
                self.error = Some(error);
                None
            }
            Event::DevicesRecovered => {
                if let Some(error) = self.last_devices_error.take()
                    && self.error.as_ref() == Some(&error)
                {
                    self.error = None;
                }
                None
            }
            Event::Started {
                request_id,
                latency_ms,
            } if request_id == self.request_id && self.phase == Phase::Starting => {
                self.phase = Phase::Running;
                self.latency_ms = if latency_ms.is_finite() {
                    latency_ms.max(0.0)
                } else {
                    0.0
                };
                self.error = None;
                None
            }
            Event::Stopped { request_id }
                if request_id == self.request_id && self.phase == Phase::Stopping =>
            {
                self.phase = Phase::Stopped;
                self.latency_ms = 0.0;
                None
            }
            Event::Failed { request_id, error }
                if request_id == self.request_id && self.phase != Phase::Stopped =>
            {
                self.phase = Phase::Stopped;
                self.latency_ms = 0.0;
                self.error = Some(error);
                None
            }
            // A cancelled start, old route failure, or duplicate acknowledgement.
            _ => None,
        }
    }

    fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Starting | Phase::Running)
    }

    fn next_request(&mut self) -> u64 {
        self.request_id = self.request_id.wrapping_add(1);
        self.request_id
    }

    fn start(&mut self) -> Option<Command> {
        let (Some(input), Some(output)) = (self.input, self.output) else {
            self.error = Some("Select a microphone and speaker before starting.".into());
            return None;
        };
        if !contains(&self.devices.inputs, input) || !contains(&self.devices.outputs, output) {
            self.error = Some("The selected audio route is no longer available.".into());
            return None;
        }
        self.phase = Phase::Starting;
        self.latency_ms = 0.0;
        self.error = None;
        Some(Command::Start {
            request_id: self.next_request(),
            input,
            output,
            gain: self.gain,
            muted: self.muted,
        })
    }

    fn stop(&mut self) -> Command {
        self.phase = Phase::Stopping;
        self.latency_ms = 0.0;
        Command::Stop {
            request_id: self.next_request(),
        }
    }

    fn route_changed(&mut self) -> Option<Command> {
        if !self.is_active() {
            return None;
        }
        if self.input.is_none() || self.output.is_none() {
            Some(self.stop())
        } else {
            self.start()
        }
    }

    fn refresh_devices(&mut self, devices: Devices) -> Option<Command> {
        let input_lost = self.input.is_some_and(|id| !contains(&devices.inputs, id));
        let output_lost = self
            .output
            .is_some_and(|id| !contains(&devices.outputs, id));
        if input_lost {
            self.input = None;
            self.auto_input = false;
        }
        if output_lost {
            self.output = None;
            self.auto_output = false;
        }
        // Explicit None (including a disconnected selection) remains None.
        if self.input.is_none() && self.auto_input {
            self.input = preferred_input(&devices.inputs);
        }
        if self.output.is_none() && self.auto_output {
            self.output = preferred_output(&devices.outputs);
        }
        self.devices = devices;
        if (input_lost || output_lost) && self.is_active() {
            self.error = Some(DEVICE_LOST.into());
            return Some(self.stop());
        }
        None
    }
}

fn contains(devices: &[AudioDevice], id: u32) -> bool {
    devices.iter().any(|device| device.id == id)
}

fn named(device: &AudioDevice, word: &str) -> bool {
    device.name.to_lowercase().contains(word)
}

fn preferred_input(devices: &[AudioDevice]) -> Option<u32> {
    devices
        .iter()
        .find(|device| device.is_continuity)
        .or_else(|| devices.iter().find(|device| named(device, "cell")))
        .or_else(|| devices.iter().find(|device| !device.is_built_in))
        .or_else(|| devices.first())
        .map(|device| device.id)
}

fn preferred_output(devices: &[AudioDevice]) -> Option<u32> {
    devices
        .iter()
        .find(|device| device.is_built_in && named(device, "speaker"))
        .or_else(|| devices.iter().find(|device| named(device, "speaker")))
        .or_else(|| {
            devices
                .iter()
                .find(|device| !device.is_bluetooth && !device.is_continuity)
        })
        .or_else(|| devices.first())
        .map(|device| device.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(
        id: u32,
        name: &str,
        built_in: bool,
        bluetooth: bool,
        continuity: bool,
    ) -> AudioDevice {
        AudioDevice {
            id,
            name: name.into(),
            is_built_in: built_in,
            is_bluetooth: bluetooth,
            is_continuity: continuity,
        }
    }

    fn devices() -> Devices {
        Devices {
            inputs: vec![
                device(1, "Built-in Microphone", true, false, false),
                device(2, "USB microphone", false, false, false),
            ],
            outputs: vec![
                device(3, "Built-in Speakers", true, false, false),
                device(4, "USB headphones", false, false, false),
            ],
        }
    }

    fn ready() -> Controller {
        let mut controller = Controller::default();
        assert!(controller.apply(Event::Devices(devices())).is_none());
        controller
    }

    fn start(controller: &mut Controller) -> u64 {
        let Some(Command::Start { request_id, .. }) = controller.toggle_running() else {
            panic!("expected start command");
        };
        request_id
    }

    fn running() -> Controller {
        let mut controller = ready();
        let request_id = start(&mut controller);
        controller.apply(Event::Started {
            request_id,
            latency_ms: 8.0,
        });
        controller
    }

    #[test]
    fn device_recovery_clears_enumeration_error_with_unchanged_or_changed_snapshot() {
        for changed in [false, true] {
            let mut controller = ready();
            controller.apply(Event::DevicesError("Device enumeration failed".into()));
            assert_eq!(
                controller.error.as_deref(),
                Some("Device enumeration failed")
            );
            assert!(controller.apply(Event::DevicesRecovered).is_none());
            if changed {
                let mut refreshed = devices();
                refreshed.inputs[0].name = "Renamed microphone".into();
                controller.apply(Event::Devices(refreshed));
            }
            assert!(controller.error.is_none());
            assert!(controller.last_devices_error.is_none());
        }
    }

    #[test]
    fn device_recovery_preserves_later_permission_and_disconnected_route_errors() {
        let mut controller = ready();
        let request_id = start(&mut controller);
        controller.apply(Event::DevicesError("Device enumeration failed".into()));
        controller.apply(Event::Failed {
            request_id,
            error: "Microphone permission denied".into(),
        });
        controller.apply(Event::DevicesRecovered);
        assert_eq!(
            controller.error.as_deref(),
            Some("Microphone permission denied")
        );
        assert!(controller.last_devices_error.is_none());

        let mut controller = running();
        controller.apply(Event::DevicesError("Device enumeration failed".into()));
        let mut refreshed = devices();
        refreshed.inputs.retain(|device| device.id != 2);
        assert!(matches!(
            controller.apply(Event::Devices(refreshed)),
            Some(Command::Stop { .. })
        ));
        let route_error = controller.error.clone();
        controller.apply(Event::DevicesRecovered);
        assert_eq!(controller.error, route_error);
        assert!(controller.last_devices_error.is_none());
    }

    #[test]
    fn lifecycle_waits_for_acknowledgements_and_ignores_repeated_toggles() {
        let mut controller = ready();
        let request_id = start(&mut controller);
        assert_eq!(controller.phase, Phase::Starting);
        assert!(controller.toggle_running().is_none());
        controller.apply(Event::Started {
            request_id,
            latency_ms: 8.0,
        });
        assert_eq!(controller.phase, Phase::Running);
        assert_eq!(controller.latency_ms, 8.0);
        let Some(Command::Stop { request_id }) = controller.toggle_running() else {
            panic!("expected stop command");
        };
        assert_eq!(controller.phase, Phase::Stopping);
        assert_eq!(controller.latency_ms, 0.0);
        assert!(controller.toggle_running().is_none());
        controller.apply(Event::Stopped { request_id });
        assert_eq!(controller.phase, Phase::Stopped);
    }

    #[test]
    fn start_and_runtime_failures_leave_a_retryable_stopped_state() {
        let mut controller = ready();
        let request_id = start(&mut controller);
        controller.apply(Event::Failed {
            request_id,
            error: "Microphone permission denied".into(),
        });
        assert_eq!(controller.phase, Phase::Stopped);
        assert_eq!(
            controller.error.as_deref(),
            Some("Microphone permission denied")
        );
        let request_id = start(&mut controller);
        assert!(controller.error.is_none());
        controller.apply(Event::Started {
            request_id,
            latency_ms: 8.0,
        });
        controller.apply(Event::Failed {
            request_id,
            error: "Audio callback failed".into(),
        });
        assert_eq!(controller.phase, Phase::Stopped);
        assert_eq!(controller.latency_ms, 0.0);
        assert_eq!(controller.error.as_deref(), Some("Audio callback failed"));
        controller.apply(Event::Started {
            request_id,
            latency_ms: 8.0,
        });
        assert_eq!(controller.phase, Phase::Stopped);
    }

    #[test]
    fn missing_route_cannot_start_and_explicit_none_survives_refresh() {
        let mut controller = Controller::default();
        assert!(controller.toggle_running().is_none());
        assert!(controller.error.is_some());
        controller.apply(Event::Devices(devices()));
        controller.select_input(None);
        controller.select_output(None);
        controller.apply(Event::Devices(devices()));
        assert_eq!(controller.input, None);
        assert_eq!(controller.output, None);
        assert!(controller.toggle_running().is_none());
        assert_eq!(controller.phase, Phase::Stopped);
    }

    #[test]
    fn none_cancels_permission_pending_start_and_late_events_cannot_resurrect_it() {
        let mut controller = ready();
        let old_request = start(&mut controller);
        let Some(Command::Stop { request_id }) = controller.select_input(None) else {
            panic!("expected cancellation");
        };
        controller.apply(Event::Started {
            request_id: old_request,
            latency_ms: 8.0,
        });
        controller.apply(Event::Failed {
            request_id: old_request,
            error: "old permission result".into(),
        });
        assert_eq!(controller.phase, Phase::Stopping);
        assert!(controller.error.is_none());
        controller.apply(Event::Stopped { request_id });
        controller.apply(Event::Started {
            request_id: old_request,
            latency_ms: 8.0,
        });
        assert_eq!(controller.phase, Phase::Stopped);
        assert_eq!(controller.input, None);
    }

    #[test]
    fn live_route_changes_restart_with_latest_preferences_and_ignore_old_events() {
        let mut controller = running();
        controller.set_gain(1.5);
        controller.toggle_mute();
        let Some(Command::Start {
            request_id,
            input,
            output,
            gain,
            muted,
        }) = controller.select_input(Some(1))
        else {
            panic!("expected route restart");
        };
        assert_eq!((input, output, gain, muted), (1, 3, 1.5, true));
        assert!(controller.select_input(Some(1)).is_none());
        let Some(Command::Start {
            request_id: newer_request,
            output,
            ..
        }) = controller.select_output(Some(4))
        else {
            panic!("expected pending route replacement");
        };
        assert_eq!(output, 4);
        controller.apply(Event::Started {
            request_id,
            latency_ms: 99.0,
        });
        controller.apply(Event::Stopped { request_id });
        controller.apply(Event::Failed {
            request_id,
            error: "old route".into(),
        });
        assert_eq!(controller.phase, Phase::Starting);
        assert!(controller.error.is_none());
        controller.apply(Event::Started {
            request_id: newer_request,
            latency_ms: 12.0,
        });
        assert_eq!(controller.phase, Phase::Running);
        assert_eq!(controller.latency_ms, 12.0);
    }

    #[test]
    fn device_refresh_preserves_ids_even_when_metadata_and_preferences_change() {
        let mut controller = ready();
        controller.select_input(Some(1));
        controller.select_output(Some(4));
        let mut refreshed = devices();
        refreshed.inputs[0].name = "Renamed microphone".into();
        refreshed
            .inputs
            .push(device(5, "iPhone", false, false, true));
        refreshed.outputs[1].name = "Renamed headphones".into();
        controller.apply(Event::Devices(refreshed));
        assert_eq!((controller.input, controller.output), (Some(1), Some(4)));
    }

    #[test]
    fn disconnected_active_route_stops_with_error_without_switching_to_another_device() {
        for pending in [false, true] {
            let mut controller = if pending { ready() } else { running() };
            if pending {
                start(&mut controller);
            }
            let mut refreshed = devices();
            refreshed.inputs.retain(|device| device.id != 2);
            let Some(Command::Stop { request_id }) =
                controller.apply(Event::Devices(refreshed.clone()))
            else {
                panic!("expected disconnect stop");
            };
            assert_eq!(controller.input, None);
            assert_eq!(controller.phase, Phase::Stopping);
            assert!(
                controller
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("disconnected")
            );
            assert!(controller.apply(Event::Devices(refreshed)).is_none());
            controller.apply(Event::Stopped { request_id });
            assert_eq!(controller.phase, Phase::Stopped);
            assert!(controller.error.is_some());
        }
    }

    #[test]
    fn output_none_and_invalid_input_clear_the_active_route() {
        let mut controller = running();
        assert!(matches!(
            controller.select_output(None),
            Some(Command::Stop { .. })
        ));
        assert_eq!(controller.output, None);
        let mut controller = running();
        assert!(matches!(
            controller.select_input(Some(99)),
            Some(Command::Stop { .. })
        ));
        assert_eq!(controller.input, None);
        assert!(controller.error.is_some());
    }

    #[test]
    fn gain_and_mute_are_preserved_across_stop_and_restart() {
        let mut controller = ready();
        assert!(controller.set_gain(1.5).is_none());
        assert!(controller.toggle_mute().is_none());
        let Some(Command::Start {
            request_id,
            gain,
            muted,
            ..
        }) = controller.toggle_running()
        else {
            panic!("expected start");
        };
        assert_eq!((gain, muted), (1.5, true));
        controller.apply(Event::Started {
            request_id,
            latency_ms: 8.0,
        });
        assert_eq!(controller.set_gain(2.5), Some(Command::SetGain(2.0)));
        assert!(controller.set_gain(f32::NAN).is_none());
        assert_eq!(controller.toggle_mute(), Some(Command::SetMuted(false)));
        let Some(Command::Stop { request_id }) = controller.toggle_running() else {
            panic!("expected stop");
        };
        controller.apply(Event::Stopped { request_id });
        let Some(Command::Start { gain, muted, .. }) = controller.toggle_running() else {
            panic!("expected restart");
        };
        assert_eq!((gain, muted), (2.0, false));
    }

    #[test]
    fn input_preference_uses_continuity_then_cell_then_external_then_first() {
        let mut inputs = vec![
            device(1, "Built-in Microphone", true, false, false),
            device(2, "USB microphone", false, false, false),
            device(3, "iPhone Phone capture", false, false, false),
            device(4, "CELLS mic", false, false, false),
        ];
        assert_eq!(preferred_input(&inputs), Some(4));
        inputs.push(device(5, "Continuity capture", false, false, true));
        assert_eq!(preferred_input(&inputs), Some(5));
        inputs.pop();
        assert_eq!(preferred_input(&inputs), Some(4));
        inputs.pop();
        assert_eq!(preferred_input(&inputs), Some(2));
        inputs.pop();
        assert_eq!(preferred_input(&inputs), Some(2));
        inputs.pop();
        assert_eq!(preferred_input(&inputs), Some(1));
        inputs.clear();
        assert_eq!(preferred_input(&inputs), None);
    }

    #[test]
    fn output_preference_uses_built_in_speaker_then_speaker_then_wired_then_first() {
        let mut outputs = vec![
            device(1, "Bluetooth headphones", false, true, false),
            device(2, "Continuity output", false, false, true),
            device(3, "USB headphones", false, false, false),
            device(4, "External SPEAKER", false, true, false),
            device(5, "Built-in Speakers", true, false, false),
        ];
        assert_eq!(preferred_output(&outputs), Some(5));
        outputs.pop();
        assert_eq!(preferred_output(&outputs), Some(4));
        outputs.pop();
        assert_eq!(preferred_output(&outputs), Some(3));
        outputs.pop();
        assert_eq!(preferred_output(&outputs), Some(1));
        outputs.clear();
        assert_eq!(preferred_output(&outputs), None);
    }
}
