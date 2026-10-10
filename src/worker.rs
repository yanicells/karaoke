use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

use crate::audio::{self, AudioEngine, Devices};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEVICE_INTERVAL: Duration = Duration::from_millis(1500);
/// Shown when a selected device disappears while a route is active.
pub const DEVICE_LOST: &str =
    "Audio device changed or disconnected. Re-select your devices and press Start.";

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Refresh,
    Start {
        request_id: u64,
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
    },
    Stop {
        request_id: u64,
    },
    SetGain(f32),
    SetMuted(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Devices(Devices),
    DevicesError(String),
    /// Enumeration succeeded again, even if the device snapshot is unchanged.
    DevicesRecovered,
    Started {
        request_id: u64,
        latency_ms: f64,
    },
    Stopped {
        request_id: u64,
    },
    Failed {
        request_id: u64,
        error: String,
    },
}

enum WorkItem {
    Command {
        command: Command,
        cancel: Option<Arc<AtomicBool>>,
    },
    Shutdown,
}

/// Owns command transport, never the native engine or a blocking join handle.
pub struct Worker {
    commands: mpsc::Sender<WorkItem>,
    closing: Arc<AtomicBool>,
    start_cancel: Mutex<Option<Arc<AtomicBool>>>,
    finished: Receiver<()>,
}

impl Worker {
    pub fn new() -> Result<(Self, Receiver<Event>), String> {
        Self::spawn(AudioEngine::new)
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        let mut pending = self
            .start_cancel
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.closing.load(Ordering::Acquire) || self.finished.is_closed() {
            return Err("The audio worker has shut down.".into());
        }
        let cancel = match &command {
            Command::Start { .. } => {
                cancel_start(&mut pending);
                let cancel = Arc::new(AtomicBool::new(false));
                *pending = Some(Arc::clone(&cancel));
                Some(cancel)
            }
            Command::Stop { .. } => {
                cancel_start(&mut pending);
                None
            }
            _ => None,
        };
        self.commands
            .send(WorkItem::Command { command, cancel })
            .map_err(|_| "The audio worker is unavailable.".into())
    }

    /// Await `recv()` for completion. Closure also means the thread has exited.
    /// Multiple callers may await cloned receivers; only one consumes the item.
    pub fn shutdown(&mut self) -> Receiver<()> {
        self.request_shutdown();
        self.finished.clone()
    }

    fn request_shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        let mut pending = self
            .start_cancel
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cancel_start(&mut pending);
        let _ = self.commands.send(WorkItem::Shutdown);
    }

    // The factory is Send; the backend itself need not be. Construction, use,
    // stop and destruction all happen inside the same dedicated thread.
    fn spawn<B, F>(make_backend: F) -> Result<(Self, Receiver<Event>), String>
    where
        B: Backend + 'static,
        F: FnOnce() -> B + Send + 'static,
    {
        let (commands, incoming) = mpsc::channel();
        let (events, receiver) = async_channel::unbounded();
        let (finished, completion) = async_channel::bounded(1);
        let closing = Arc::new(AtomicBool::new(false));
        let thread_closing = Arc::clone(&closing);
        thread::Builder::new()
            .name("karaoke-audio".into())
            .spawn(move || {
                // Keep the completion sender outside run so panic unwinding
                // drops the backend before closing the acknowledgement channel.
                run(make_backend(), incoming, events, thread_closing);
                let _ = finished.try_send(());
            })
            .map_err(|error| format!("Could not start the audio worker: {error}"))?;
        Ok((
            Self {
                commands,
                closing,
                start_cancel: Mutex::new(None),
                finished: completion,
            },
            receiver,
        ))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

fn cancel_start(pending: &mut Option<Arc<AtomicBool>>) {
    if let Some(cancel) = pending.take() {
        cancel.store(true, Ordering::Release);
    }
}

/// Small private seam for exercising worker ordering without hardware or GPUI.
trait Backend {
    fn devices(&mut self) -> Result<Devices, String>;
    fn start(
        &mut self,
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
        cancel: &AtomicBool,
    ) -> Result<f64, String>;
    fn stop(&mut self);
    fn set_gain(&mut self, gain: f32);
    fn set_muted(&mut self, muted: bool);
    fn poll_error(&mut self) -> Option<String>;
}

impl Backend for AudioEngine {
    fn devices(&mut self) -> Result<Devices, String> {
        audio::devices()
    }

    fn start(
        &mut self,
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
        cancel: &AtomicBool,
    ) -> Result<f64, String> {
        self.start_cancellable(input, output, gain, muted, cancel)
    }

    fn stop(&mut self) {
        AudioEngine::stop(self);
    }
    fn set_gain(&mut self, gain: f32) {
        AudioEngine::set_gain(self, gain);
    }
    fn set_muted(&mut self, muted: bool) {
        AudioEngine::set_muted(self, muted);
    }
    fn poll_error(&mut self) -> Option<String> {
        AudioEngine::poll_error(self)
    }
}

struct ActiveRoute {
    request_id: u64,
    input: u32,
    output: u32,
}

fn run<B: Backend>(
    mut backend: B,
    commands: mpsc::Receiver<WorkItem>,
    events: Sender<Event>,
    closing: Arc<AtomicBool>,
) {
    let mut active: Option<ActiveRoute> = None;
    let mut snapshot: Option<Devices> = None;
    let mut devices_error: Option<String> = None;
    let mut next_devices = Instant::now();
    let mut next_poll = Instant::now() + POLL_INTERVAL;
    loop {
        if closing.load(Ordering::Acquire) || events.is_closed() {
            break;
        }
        let now = Instant::now();
        if now >= next_poll {
            if let Some(route) = &active
                && let Some(error) = backend.poll_error()
            {
                let request_id = route.request_id;
                backend.stop();
                active = None;
                if events
                    .try_send(Event::Failed { request_id, error })
                    .is_err()
                {
                    break;
                }
            }
            next_poll = Instant::now() + POLL_INTERVAL;
        }
        if now >= next_devices {
            if !refresh(
                &mut backend,
                &events,
                &mut snapshot,
                &mut devices_error,
                &mut active,
            ) {
                break;
            }
            next_devices = Instant::now() + DEVICE_INTERVAL;
        }
        let timeout = next_poll
            .min(next_devices)
            .saturating_duration_since(Instant::now());
        let work = match commands.recv_timeout(timeout) {
            Ok(work) => work,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // Closing overrides the entire queue, including starts queued before
        // permission was requested. Never drain stale starts during shutdown.
        if closing.load(Ordering::Acquire) || events.is_closed() {
            break;
        }
        let WorkItem::Command { command, cancel } = work else {
            break;
        };
        match command {
            Command::Refresh => {
                if !refresh(
                    &mut backend,
                    &events,
                    &mut snapshot,
                    &mut devices_error,
                    &mut active,
                ) {
                    break;
                }
                next_devices = Instant::now() + DEVICE_INTERVAL;
            }
            Command::Start {
                request_id,
                input,
                output,
                gain,
                muted,
            } => {
                let Some(cancel) = cancel else {
                    continue;
                };
                if cancel.load(Ordering::Acquire) {
                    continue;
                }
                backend.stop();
                active = None;
                let result = backend.start(input, output, gain, muted, &cancel);
                if closing.load(Ordering::Acquire)
                    || cancel.load(Ordering::Acquire)
                    || events.is_closed()
                {
                    backend.stop();
                    continue;
                }
                let event = match result {
                    Ok(latency_ms) => {
                        active = Some(ActiveRoute {
                            request_id,
                            input,
                            output,
                        });
                        Event::Started {
                            request_id,
                            latency_ms,
                        }
                    }
                    Err(error) => {
                        backend.stop();
                        Event::Failed { request_id, error }
                    }
                };
                if events.try_send(event).is_err() {
                    break;
                }
                next_poll = Instant::now() + POLL_INTERVAL;
            }
            Command::Stop { request_id } => {
                backend.stop();
                active = None;
                if events.try_send(Event::Stopped { request_id }).is_err() {
                    break;
                }
            }
            Command::SetGain(gain) => backend.set_gain(gain),
            Command::SetMuted(muted) => backend.set_muted(muted),
        }
    }
    backend.stop();
    // Explicit drop before run returns and the outer closure acknowledges quit.
    drop(backend);
}

fn refresh<B: Backend>(
    backend: &mut B,
    events: &Sender<Event>,
    snapshot: &mut Option<Devices>,
    last_error: &mut Option<String>,
    active: &mut Option<ActiveRoute>,
) -> bool {
    match backend.devices() {
        Ok(devices) => {
            let disconnected = active
                .as_ref()
                .filter(|route| {
                    !devices.inputs.iter().any(|device| device.id == route.input)
                        || !devices
                            .outputs
                            .iter()
                            .any(|device| device.id == route.output)
                })
                .map(|route| route.request_id);
            // Stop natively now; do not depend on how soon GPUI consumes events.
            if disconnected.is_some() {
                backend.stop();
                *active = None;
            }
            if last_error.take().is_some() && events.try_send(Event::DevicesRecovered).is_err() {
                return false;
            }
            if snapshot.as_ref() != Some(&devices) {
                *snapshot = Some(devices.clone());
                if events.try_send(Event::Devices(devices)).is_err() {
                    return false;
                }
            }
            if let Some(request_id) = disconnected {
                return events
                    .try_send(Event::Failed {
                        request_id,
                        error: DEVICE_LOST.into(),
                    })
                    .is_ok();
            }
            true
        }
        Err(error) => {
            if last_error.as_ref() != Some(&error) {
                *last_error = Some(error.clone());
                return events.try_send(Event::DevicesError(error)).is_ok();
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioDevice;
    use std::sync::atomic::AtomicUsize;

    struct State {
        starts: AtomicUsize,
        stopped: AtomicBool,
        dropped: AtomicBool,
        snapshot: Mutex<Devices>,
        devices_error: Mutex<Option<String>>,
        error: Mutex<Option<String>>,
        thread: Mutex<Option<thread::ThreadId>>,
        calls: Mutex<Vec<(f32, bool)>>,
    }

    struct FakeBackend {
        state: Arc<State>,
        entered: mpsc::Sender<()>,
        permission: Option<mpsc::Receiver<()>>,
    }

    impl Backend for FakeBackend {
        fn devices(&mut self) -> Result<Devices, String> {
            assert_eq!(
                Some(thread::current().id()),
                *self.state.thread.lock().unwrap()
            );
            if let Some(error) = self.state.devices_error.lock().unwrap().clone() {
                return Err(error);
            }
            Ok(self.state.snapshot.lock().unwrap().clone())
        }

        fn start(
            &mut self,
            _: u32,
            _: u32,
            gain: f32,
            muted: bool,
            cancel: &AtomicBool,
        ) -> Result<f64, String> {
            self.state.starts.fetch_add(1, Ordering::SeqCst);
            self.state.calls.lock().unwrap().push((gain, muted));
            let _ = self.entered.send(());
            if let Some(permission) = self.permission.take() {
                let _ = permission.recv_timeout(Duration::from_secs(2));
            }
            if cancel.load(Ordering::Acquire) {
                return Err("Start cancelled".into());
            }
            self.state.stopped.store(false, Ordering::SeqCst);
            Ok(8.0)
        }

        fn stop(&mut self) {
            self.state.stopped.store(true, Ordering::SeqCst);
        }
        fn set_gain(&mut self, _: f32) {}
        fn set_muted(&mut self, _: bool) {}
        fn poll_error(&mut self) -> Option<String> {
            self.state.error.lock().unwrap().take()
        }
    }

    impl Drop for FakeBackend {
        fn drop(&mut self) {
            assert!(self.state.stopped.load(Ordering::SeqCst));
            assert_eq!(
                Some(thread::current().id()),
                *self.state.thread.lock().unwrap()
            );
            self.state.dropped.store(true, Ordering::SeqCst);
        }
    }

    struct Harness {
        worker: Worker,
        events: Receiver<Event>,
        state: Arc<State>,
        entered: mpsc::Receiver<()>,
        permission: mpsc::Sender<()>,
    }

    fn harness(block_permission: bool) -> Harness {
        let device = |id| AudioDevice {
            id,
            name: format!("Device {id}"),
            is_built_in: true,
            is_bluetooth: false,
            is_continuity: false,
        };
        let state = Arc::new(State {
            starts: AtomicUsize::new(0),
            stopped: AtomicBool::new(true),
            dropped: AtomicBool::new(false),
            snapshot: Mutex::new(Devices {
                inputs: vec![device(1)],
                outputs: vec![device(2)],
            }),
            devices_error: Mutex::new(None),
            error: Mutex::new(None),
            thread: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        });
        let (entered, entry) = mpsc::channel();
        let (permission, granted) = mpsc::channel();
        let backend_state = Arc::clone(&state);
        let (worker, events) = Worker::spawn(move || {
            *backend_state.thread.lock().unwrap() = Some(thread::current().id());
            FakeBackend {
                state: backend_state,
                entered,
                permission: block_permission.then_some(granted),
            }
        })
        .unwrap();
        Harness {
            worker,
            events,
            state,
            entered: entry,
            permission,
        }
    }

    fn start(request_id: u64) -> Command {
        Command::Start {
            request_id,
            input: 1,
            output: 2,
            gain: 1.5,
            muted: true,
        }
    }

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(Instant::now() < deadline, "timed out waiting for worker");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn event(events: &Receiver<Event>) -> Event {
        let mut result = None;
        wait_until(|| {
            result = events.try_recv().ok();
            result.is_some()
        });
        result.unwrap()
    }

    fn finish(worker: &mut Worker, state: &State) {
        let completion = worker.shutdown();
        wait_until(|| completion.try_recv().is_ok() || completion.is_closed());
        assert!(state.stopped.load(Ordering::SeqCst));
        assert!(state.dropped.load(Ordering::SeqCst));
        assert!(worker.send(Command::Refresh).is_err());
    }

    #[test]
    fn shutdown_cancels_blocked_permission_and_acknowledges_after_engine_drop() {
        let mut h = harness(true);
        assert!(matches!(event(&h.events), Event::Devices(_)));
        h.worker.send(start(1)).unwrap();
        h.entered.recv_timeout(Duration::from_secs(2)).unwrap();
        h.worker.send(start(2)).unwrap();
        let completion = h.worker.shutdown();
        assert!(matches!(
            completion.try_recv(),
            Err(async_channel::TryRecvError::Empty)
        ));
        assert!(!h.state.dropped.load(Ordering::SeqCst));
        h.permission.send(()).unwrap();
        wait_until(|| completion.try_recv().is_ok() || completion.is_closed());
        assert!(h.state.dropped.load(Ordering::SeqCst));
        assert_eq!(h.state.starts.load(Ordering::SeqCst), 1);
        assert!(h.events.try_recv().is_err());
        assert!(h.worker.send(start(3)).is_err());
    }

    #[test]
    fn stop_cancels_inflight_and_all_queued_starts() {
        let mut h = harness(true);
        event(&h.events);
        h.worker.send(start(1)).unwrap();
        h.entered.recv_timeout(Duration::from_secs(2)).unwrap();
        h.worker.send(start(2)).unwrap();
        h.worker.send(start(3)).unwrap();
        h.worker.send(Command::Stop { request_id: 4 }).unwrap();
        h.permission.send(()).unwrap();
        assert_eq!(event(&h.events), Event::Stopped { request_id: 4 });
        assert_eq!(h.state.starts.load(Ordering::SeqCst), 1);
        assert!(h.state.stopped.load(Ordering::SeqCst));
        finish(&mut h.worker, &h.state);
    }

    #[test]
    fn superseding_a_blocked_start_runs_only_the_latest_route_request() {
        let mut h = harness(true);
        event(&h.events);
        h.worker.send(start(1)).unwrap();
        h.entered.recv_timeout(Duration::from_secs(2)).unwrap();
        h.worker.send(start(2)).unwrap();
        h.worker
            .send(Command::Start {
                request_id: 3,
                input: 1,
                output: 2,
                gain: 0.5,
                muted: false,
            })
            .unwrap();
        h.permission.send(()).unwrap();
        assert_eq!(
            event(&h.events),
            Event::Started {
                request_id: 3,
                latency_ms: 8.0
            }
        );
        assert_eq!(h.state.starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            *h.state.calls.lock().unwrap(),
            vec![(1.5, true), (0.5, false)]
        );
        finish(&mut h.worker, &h.state);
    }

    #[test]
    fn drop_requests_shutdown_without_waiting_for_permission() {
        let h = harness(true);
        event(&h.events);
        h.worker.send(start(1)).unwrap();
        h.entered.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(h.worker);
        assert!(!h.state.dropped.load(Ordering::SeqCst));
        h.permission.send(()).unwrap();
        wait_until(|| h.state.dropped.load(Ordering::SeqCst));
        assert_eq!(h.state.starts.load(Ordering::SeqCst), 1);
        assert!(h.events.try_recv().is_err());
    }

    #[test]
    fn receiver_closure_stops_native_audio_and_releases_worker() {
        let mut h = harness(false);
        event(&h.events);
        h.worker.send(start(7)).unwrap();
        assert_eq!(
            event(&h.events),
            Event::Started {
                request_id: 7,
                latency_ms: 8.0
            }
        );
        assert_eq!(*h.state.calls.lock().unwrap(), vec![(1.5, true)]);
        assert_ne!(
            *h.state.thread.lock().unwrap(),
            Some(thread::current().id())
        );
        drop(h.events);
        wait_until(|| h.state.dropped.load(Ordering::SeqCst));
        finish(&mut h.worker, &h.state);
    }

    #[test]
    fn active_errors_stop_engine_and_are_reported_once() {
        let mut h = harness(false);
        event(&h.events);
        h.worker.send(start(9)).unwrap();
        event(&h.events);
        *h.state.error.lock().unwrap() = Some("Render callback failed".into());
        assert_eq!(
            event(&h.events),
            Event::Failed {
                request_id: 9,
                error: "Render callback failed".into()
            }
        );
        assert!(h.state.stopped.load(Ordering::SeqCst));
        finish(&mut h.worker, &h.state);
        assert!(h.events.try_recv().is_err());
    }

    #[test]
    fn refresh_reports_recovery_once_with_unchanged_or_changed_snapshot() {
        for changed in [false, true] {
            let mut h = harness(false);
            let Event::Devices(initial) = event(&h.events) else {
                panic!("expected initial snapshot");
            };
            *h.state.devices_error.lock().unwrap() = Some("Device enumeration failed".into());
            h.worker.send(Command::Refresh).unwrap();
            assert_eq!(
                event(&h.events),
                Event::DevicesError("Device enumeration failed".into())
            );
            h.worker.send(Command::Refresh).unwrap();
            // A stop acknowledgement marks completion of the preceding refresh.
            h.worker.send(Command::Stop { request_id: 1 }).unwrap();
            assert_eq!(event(&h.events), Event::Stopped { request_id: 1 });

            if changed {
                h.state.snapshot.lock().unwrap().inputs[0].name = "Renamed microphone".into();
            }
            *h.state.devices_error.lock().unwrap() = None;
            h.worker.send(Command::Refresh).unwrap();
            assert_eq!(event(&h.events), Event::DevicesRecovered);
            if changed {
                let Event::Devices(refreshed) = event(&h.events) else {
                    panic!("expected changed snapshot");
                };
                assert_ne!(refreshed, initial);
            }
            h.worker.send(Command::Refresh).unwrap();
            h.worker.send(Command::Stop { request_id: 2 }).unwrap();
            assert_eq!(event(&h.events), Event::Stopped { request_id: 2 });
            finish(&mut h.worker, &h.state);
            assert!(h.events.try_recv().is_err());
        }
    }

    #[test]
    fn refresh_deduplicates_snapshots_and_stops_disconnected_audio_before_ui_handles_event() {
        let mut h = harness(false);
        event(&h.events);
        h.worker.send(Command::Refresh).unwrap();
        h.worker.send(start(10)).unwrap();
        assert_eq!(
            event(&h.events),
            Event::Started {
                request_id: 10,
                latency_ms: 8.0
            }
        );
        h.state.snapshot.lock().unwrap().outputs.clear();
        h.worker.send(Command::Refresh).unwrap();
        let Event::Devices(devices) = event(&h.events) else {
            panic!("expected changed snapshot");
        };
        assert!(devices.outputs.is_empty());
        assert!(h.state.stopped.load(Ordering::SeqCst));
        assert!(matches!(
            event(&h.events),
            Event::Failed { request_id: 10, .. }
        ));
        h.worker.send(Command::Refresh).unwrap();
        finish(&mut h.worker, &h.state);
        assert!(h.events.try_recv().is_err());
    }
}
