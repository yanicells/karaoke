//! Explicit, silent hardware verification; it never requests or resets permission.
use std::time::{Duration, Instant};

use crate::{audio, controller::Controller, worker::Event};

pub fn probe(arguments: Vec<String>) -> Result<(), String> {
    if audio::permission_status() != "authorized" {
        return Err("Microphone access is not authorized. Open KaraokeMic.app and press Start to grant access, then retry the muted probe.".into());
    }
    let (input, output, duration) = match arguments.as_slice() {
        [] => {
            let mut controller = Controller::default();
            controller.apply(Event::Devices(audio::devices()?));
            (
                controller.input.ok_or("No microphone is available.")?,
                controller.output.ok_or("No speaker is available.")?,
                Duration::from_secs(3),
            )
        }
        [input, output, duration] => {
            let input = input
                .parse()
                .map_err(|_| "Input ID must be an unsigned integer.")?;
            let output = output
                .parse()
                .map_err(|_| "Output ID must be an unsigned integer.")?;
            let milliseconds = duration
                .parse::<u64>()
                .map_err(|_| "Duration must be milliseconds.")?;
            if !(1000..=60_000).contains(&milliseconds) {
                return Err("Probe duration must be between 1000 and 60000 milliseconds.".into());
            }
            (input, output, Duration::from_millis(milliseconds))
        }
        _ => return Err("Usage: --probe-muted [input-id output-id duration-ms]".into()),
    };
    let mut engine = audio::AudioEngine::new();
    let latency = engine.start(input, output, 1., true)?;
    println!("Muted route {input} → {output}; estimated latency {latency:.2} ms");
    let start = Instant::now();
    while start.elapsed() < duration {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(error) = engine.poll_error() {
            return Err(error);
        }
        // Gain changes must remain silent throughout hardware verification.
        engine.set_muted(true);
        engine.set_gain(1.5);
    }
    let diagnostics = engine.diagnostics();
    engine.stop();
    println!("{diagnostics:?}");
    if diagnostics.input_frames == 0 || diagnostics.output_frames == 0 {
        return Err("The route started but did not process both input and output audio.".into());
    }
    println!("Muted probe passed. Audible output and physical latency need separate verification.");
    Ok(())
}
