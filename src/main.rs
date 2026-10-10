#[cfg(not(target_os = "macos"))]
compile_error!("KaraokeMic uses native macOS CoreAudio and requires macOS 14 or newer.");

mod audio;
mod controller;
mod diagnostics;
mod ui;
mod worker;

use gpui::{Application, AssetSource, SharedString};
use std::borrow::Cow;

struct Assets;

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}: {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "microphone.svg" => Some(Cow::Borrowed(include_bytes!("../assets/logo.svg"))),
            _ => None,
        })
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec!["microphone.svg".into()])
    }
}

fn main() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    if let Some(argument) = std::env::args().nth(1) {
        match argument.as_str() {
            "--list-devices" => {
                println!("Microphone permission: {}", audio::permission_status());
                match audio::devices() {
                    Ok(devices) => {
                        for (label, devices) in
                            [("Inputs", devices.inputs), ("Outputs", devices.outputs)]
                        {
                            println!("{label}:");
                            for device in devices {
                                println!(
                                    "  {}: {}{}{}",
                                    device.id,
                                    device.name,
                                    if device.is_continuity { " [phone]" } else { "" },
                                    if device.is_bluetooth {
                                        " [Bluetooth]"
                                    } else {
                                        ""
                                    }
                                );
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            "--help" | "-h" => {
                println!(
                    "KaraokeMic — live microphone monitor\n\n  --list-devices  List audio devices and permission status\n  --probe-muted [input-id output-id duration-ms]\n                  Verify a silent audio route with existing mic permission\n  --help          Show this help\n\nRun the app bundle for microphone monitoring."
                );
                return;
            }
            "--probe-muted" => {
                if let Err(error) = diagnostics::probe(std::env::args().skip(2).collect()) {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
                return;
            }
            _ => {
                eprintln!("Unknown option: {argument}");
                std::process::exit(2);
            }
        }
    }
    Application::new().with_assets(Assets).run(ui::open);
}
