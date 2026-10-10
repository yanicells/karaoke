use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, FocusHandle, ImageSource, KeyBinding, KeyDownEvent, Menu, MenuItem,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, Resource, ScrollHandle,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, canvas, div,
    img, prelude::*, px, relative, rgb, size,
};

use crate::{
    controller::{Controller, Phase},
    worker::{Command, Event, Worker},
};

const INK: u32 = 0x21334B;
const SECONDARY: u32 = 0x65748A;
const BLUE: u32 = 0x496B94;
const PALE: u32 = 0xF0F5FC;
const LINE: u32 = 0xDCE4EE;
const RED: u32 = 0xB33E4A;

actions!(karaoke, [Quit, RefreshDevices]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Input,
    Output,
    Gain,
    Start,
    Mute,
}

const CONTROLS: [Control; 5] = [
    Control::Input,
    Control::Output,
    Control::Gain,
    Control::Start,
    Control::Mute,
];

pub fn open(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-r", RefreshDevices, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "KaraokeMic".into(),
            items: vec![MenuItem::action("Quit KaraokeMic", Quit)],
        },
        Menu {
            name: "Audio".into(),
            items: vec![MenuItem::action("Refresh devices", RefreshDevices)],
        },
    ]);
    cx.on_window_closed(|cx| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    let bounds = Bounds::centered(None, size(px(390.), px(535.)), cx);
    if let Err(error) = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(390.), px(535.))),
            titlebar: Some(TitlebarOptions {
                title: Some("KaraokeMic".into()),
                ..Default::default()
            }),
            is_resizable: false,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| MicView::new(window, cx)),
    ) {
        eprintln!("Could not open KaraokeMic: {error}");
        cx.quit();
    }
    cx.activate(true);
}

struct MicView {
    state: Controller,
    worker: Option<Worker>,
    focus: FocusHandle,
    focused: Control,
    keyboard_focus: bool,
    picker: Option<Control>,
    picker_index: usize,
    picker_scroll: ScrollHandle,
    picker_bounds: [Rc<Cell<Bounds<Pixels>>>; 2],
    gain_bounds: Rc<Cell<Bounds<Pixels>>>,
    dragging_gain: bool,
    _events: Option<Task<()>>,
    _quit: Subscription,
}

impl MicView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut state = Controller::default();
        let (worker, events) = match Worker::new() {
            Ok((worker, events)) => {
                let task = cx.spawn(async move |this, cx| {
                    while let Ok(event) = events.recv().await {
                        if this
                            .update(cx, |view, cx| {
                                if matches!(&event, Event::Devices(_)) {
                                    view.picker = None;
                                }
                                let command = view.state.apply(event);
                                view.send(command, cx);
                                cx.notify();
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                (Some(worker), Some(task))
            }
            Err(error) => {
                state.error = Some(error);
                (None, None)
            }
        };
        let quit = cx.on_app_quit(|view, _| {
            let done = view.worker.as_mut().map(Worker::shutdown);
            async move {
                if let Some(done) = done {
                    let _ = done.recv().await;
                }
            }
        });
        Self {
            state,
            worker,
            focus,
            focused: Control::Start,
            keyboard_focus: false,
            picker: None,
            picker_index: 0,
            picker_scroll: ScrollHandle::new(),
            picker_bounds: std::array::from_fn(|_| Rc::new(Cell::new(Bounds::default()))),
            gain_bounds: Rc::new(Cell::new(Bounds::default())),
            dragging_gain: false,
            _events: events,
            _quit: quit,
        }
    }

    fn send(&mut self, command: Option<Command>, cx: &mut Context<Self>) {
        if let Some(command) = command {
            let result = self
                .worker
                .as_ref()
                .ok_or_else(|| {
                    "Audio worker is unavailable. Quit and reopen KaraokeMic.".to_string()
                })
                .and_then(|worker| worker.send(command));
            if let Err(error) = result {
                self.state.phase = Phase::Stopped;
                self.state.latency_ms = 0.;
                self.state.error = Some(error);
            }
        }
        // Actions can update an error or stopped-state gain without a worker command.
        cx.notify();
    }

    fn busy(&self) -> bool {
        matches!(self.state.phase, Phase::Starting | Phase::Stopping)
    }

    fn choose(&mut self, control: Control, id: Option<u32>, cx: &mut Context<Self>) {
        let command = if control == Control::Input {
            self.state.select_input(id)
        } else {
            self.state.select_output(id)
        };
        self.picker = None;
        self.send(command, cx);
    }

    fn toggle_picker(&mut self, control: Control, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.focused = control;
        self.picker = if self.picker == Some(control) {
            None
        } else {
            Some(control)
        };
        let (devices, selected) = if control == Control::Input {
            (&self.state.devices.inputs, self.state.input)
        } else {
            (&self.state.devices.outputs, self.state.output)
        };
        self.picker_index = devices
            .iter()
            .position(|device| Some(device.id) == selected)
            .map_or(0, |index| index + 1);
        self.picker_scroll.scroll_to_item(self.picker_index);
        cx.notify();
    }

    fn gain_at(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let bounds = self.gain_bounds.get();
        let width = f32::from(bounds.size.width);
        if width > 0. {
            let fraction = f32::from(position.x - bounds.origin.x) / width;
            let gain = ((fraction * 40.).round() / 20.).clamp(0., 2.);
            let command = self.state.set_gain(gain);
            self.send(command, cx);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.platform || event.keystroke.modifiers.control {
            return;
        }
        self.keyboard_focus = true;
        let key = event.keystroke.key.as_str();
        if key == "tab" {
            self.picker = None;
            let current = CONTROLS
                .iter()
                .position(|control| *control == self.focused)
                .unwrap_or(0);
            let step = if event.keystroke.modifiers.shift {
                4
            } else {
                1
            };
            let mut next = (current + step) % CONTROLS.len();
            if CONTROLS[next] == Control::Mute && self.state.phase != Phase::Running {
                next = (next + step) % CONTROLS.len();
            }
            self.focused = CONTROLS[next];
        } else if key == "escape" {
            self.picker = None;
        } else if let Some(control) = self.picker {
            let devices = if control == Control::Input {
                &self.state.devices.inputs
            } else {
                &self.state.devices.outputs
            };
            match key {
                "up" => self.picker_index = self.picker_index.saturating_sub(1),
                "down" => self.picker_index = (self.picker_index + 1).min(devices.len()),
                "enter" | "space" => {
                    if self.picker_index > devices.len() {
                        self.picker = None;
                        cx.notify();
                        return;
                    }
                    let id = self
                        .picker_index
                        .checked_sub(1)
                        .and_then(|index| devices.get(index))
                        .map(|d| d.id);
                    self.choose(control, id, cx);
                }
                _ => return,
            }
            self.picker_scroll.scroll_to_item(self.picker_index);
        } else {
            match (self.focused, key) {
                (Control::Input | Control::Output, "enter" | "space" | "down" | "up") => {
                    self.toggle_picker(self.focused, cx);
                }
                (Control::Gain, "left" | "down") => {
                    let command = self.state.set_gain(self.state.gain - 0.05);
                    self.send(command, cx);
                }
                (Control::Gain, "right" | "up") => {
                    let command = self.state.set_gain(self.state.gain + 0.05);
                    self.send(command, cx);
                }
                (Control::Start, "enter" | "space") => {
                    let command = self.state.toggle_running();
                    self.send(command, cx);
                }
                (Control::Mute, "enter" | "space") | (_, "m") => {
                    if self.state.phase == Phase::Running {
                        let command = self.state.toggle_mute();
                        self.send(command, cx);
                    }
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn border(&self, control: Control) -> gpui::Rgba {
        rgb(if self.keyboard_focus && self.focused == control {
            BLUE
        } else {
            LINE
        })
    }

    fn device_picker(&self, control: Control, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, devices, selected, index) = if control == Control::Input {
            (
                "Microphone",
                &self.state.devices.inputs,
                self.state.input,
                0,
            )
        } else {
            ("Speaker", &self.state.devices.outputs, self.state.output, 1)
        };
        let selected = devices.iter().find(|device| Some(device.id) == selected);
        let name = selected
            .map_or("None", |device| device.name.as_str())
            .to_owned();
        let bluetooth = selected.is_some_and(|device| device.is_bluetooth);
        let bounds = self.picker_bounds[index].clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .id(if index == 0 {
                        "input-picker"
                    } else {
                        "output-picker"
                    })
                    .relative()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(42.))
                    .px_3()
                    .rounded_lg()
                    .bg(rgb(0xF8FAFD))
                    .border_1()
                    .border_color(self.border(control))
                    .cursor_pointer()
                    .when(self.busy(), |this| this.opacity(0.5))
                    .hover(|style| style.bg(rgb(PALE)))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.keyboard_focus = false;
                        view.toggle_picker(control, cx);
                    }))
                    .child(div().flex_1().overflow_hidden().text_ellipsis().child(name))
                    .when(bluetooth, |this| {
                        this.child(
                            div()
                                .ml_2()
                                .text_size(px(10.))
                                .text_color(rgb(SECONDARY))
                                .child("Bluetooth"),
                        )
                    })
                    .child(div().ml_2().text_color(rgb(SECONDARY)).child("⌄"))
                    .child(
                        canvas(
                            move |rect, _, _| {
                                bounds.set(rect);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    ),
            )
    }

    fn popup(&self, control: Control, cx: &mut Context<Self>) -> impl IntoElement {
        let index = usize::from(control == Control::Output);
        let bounds = self.picker_bounds[index].get();
        let devices = if index == 0 {
            &self.state.devices.inputs
        } else {
            &self.state.devices.outputs
        };
        let options = std::iter::once((None, "None".to_string()))
            .chain(devices.iter().map(|device| {
                let suffix = if device.is_bluetooth {
                    " · Bluetooth"
                } else {
                    ""
                };
                (Some(device.id), format!("{}{suffix}", device.name))
            }))
            .enumerate()
            .map(|(option, (id, label))| {
                div()
                    .id(("device-option", option))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(option == self.picker_index, |this| this.bg(rgb(PALE)))
                    .hover(|style| style.bg(rgb(PALE)))
                    .child(label)
                    .on_click(cx.listener(move |view, _, _, cx| view.choose(control, id, cx)))
            });
        div()
            .id("device-menu")
            .absolute()
            .left(px(24.))
            .right(px(24.))
            .top(bounds.bottom() + px(4.))
            .max_h(px(170.))
            .overflow_y_scroll()
            .track_scroll(&self.picker_scroll)
            .p_1()
            .bg(rgb(0xFFFFFF))
            .border_1()
            .border_color(rgb(LINE))
            .rounded_lg()
            .shadow_md()
            .occlude()
            .on_mouse_down_out(cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                if !view.picker_bounds[index].get().contains(&event.position) {
                    view.picker = None;
                    cx.notify();
                }
            }))
            .children(options)
    }
}

impl Render for MicView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.state.phase == Phase::Running;
        let (status, status_color) = match self.state.phase {
            Phase::Starting => ("Starting…", BLUE),
            Phase::Stopping => ("Stopping…", SECONDARY),
            Phase::Running if self.state.muted => ("Muted", SECONDARY),
            Phase::Running => ("Live", 0x2E806C),
            Phase::Stopped => ("Ready", SECONDARY),
        };
        let bluetooth = self
            .state
            .devices
            .outputs
            .iter()
            .any(|device| Some(device.id) == self.state.output && device.is_bluetooth);
        let button_label = match self.state.phase {
            Phase::Running => "Stop monitoring",
            Phase::Starting => "Starting…",
            Phase::Stopping => "Stopping…",
            Phase::Stopped => "Start monitoring",
        };
        let gain_bounds = self.gain_bounds.clone();
        let mut content = div()
            .id("karaoke-mic")
            .relative()
            .size_full()
            .p(px(24.))
            .bg(rgb(0xFFFFFF))
            .text_color(rgb(INK))
            .font_family(".SystemUIFont")
            .text_size(px(14.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(
                cx.listener(|view, _: &RefreshDevices, _, cx| {
                    view.send(Some(Command::Refresh), cx)
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                if view.dragging_gain && event.dragging() {
                    view.gain_at(event.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.dragging_gain = false),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        img(ImageSource::Resource(Resource::Embedded(
                            "microphone.svg".into(),
                        )))
                        .size(px(50.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(21.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("KaraokeMic"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(SECONDARY))
                                    .child("Live microphone monitor"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.device_picker(Control::Input, cx))
                    .child(self.device_picker(Control::Output, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Mic volume"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(SECONDARY))
                                    .child(format!("{:.0}%", self.state.gain * 100.)),
                            ),
                    )
                    .child(
                        div()
                            .id("gain-slider")
                            .relative()
                            .h(px(22.))
                            .w_full()
                            .cursor_pointer()
                            .rounded_md()
                            .border_1()
                            .border_color(if self.keyboard_focus && self.focused == Control::Gain {
                                rgb(BLUE)
                            } else {
                                rgb(0xFFFFFF)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                                    view.keyboard_focus = false;
                                    view.focused = Control::Gain;
                                    view.dragging_gain = true;
                                    view.gain_at(event.position, cx);
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(px(8.))
                                    .h(px(4.))
                                    .rounded_full()
                                    .bg(rgb(LINE)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top(px(8.))
                                    .h(px(4.))
                                    .w(relative(self.state.gain / 2.))
                                    .rounded_full()
                                    .bg(rgb(BLUE)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(self.state.gain / 2.))
                                    .ml(px(-8.))
                                    .top(px(2.))
                                    .size(px(16.))
                                    .rounded_full()
                                    .bg(rgb(0xFFFFFF))
                                    .border_2()
                                    .border_color(rgb(BLUE))
                                    .shadow_sm(),
                            )
                            .child(
                                canvas(
                                    move |bounds, _, _| gain_bounds.set(bounds),
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(10.))
                            .text_color(rgb(SECONDARY))
                            .child("0%")
                            .child("200%"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("start-stop")
                                    .flex_1()
                                    .h(px(44.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .bg(rgb(if running { RED } else { BLUE }))
                                    .text_color(rgb(0xFFFFFF))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .border_2()
                                    .border_color(
                                        if self.keyboard_focus && self.focused == Control::Start {
                                            rgb(INK)
                                        } else {
                                            rgb(if running { RED } else { BLUE })
                                        },
                                    )
                                    .cursor_pointer()
                                    .when(self.busy(), |this| this.opacity(0.55))
                                    .hover(|style| style.opacity(0.9))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.keyboard_focus = false;
                                        view.focused = Control::Start;
                                        let command = view.state.toggle_running();
                                        view.send(command, cx);
                                    }))
                                    .child(button_label),
                            )
                            .child(
                                div()
                                    .id("mute")
                                    .h(px(44.))
                                    .w(px(83.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(self.border(Control::Mute))
                                    .bg(rgb(PALE))
                                    .cursor_pointer()
                                    .when(!running, |this| this.opacity(0.45))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        if view.state.phase != Phase::Running {
                                            return;
                                        }
                                        view.keyboard_focus = false;
                                        view.focused = Control::Mute;
                                        let command = view.state.toggle_mute();
                                        view.send(command, cx);
                                    }))
                                    .child(if self.state.muted { "Unmute" } else { "Mute" }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(6.)).rounded_full().bg(rgb(status_color)))
                                    .child(div().text_color(rgb(SECONDARY)).child(status)),
                            )
                            .when(running, |this| {
                                this.child(
                                    div().text_color(rgb(SECONDARY)).child(format!(
                                        "~{:.0} ms estimated",
                                        self.state.latency_ms
                                    )),
                                )
                            }),
                    )
                    .when(running && bluetooth, |this| {
                        this.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0x8B640C))
                                .child("Bluetooth adds delay. Wired speakers respond faster."),
                        )
                    }),
            )
            .when_some(self.state.error.clone(), |this, error| {
                this.child(
                    div()
                        .id("audio-error")
                        .max_h(px(55.))
                        .overflow_y_scroll()
                        .text_size(px(11.))
                        .text_color(rgb(RED))
                        .child(error),
                )
            })
            .child(div().flex_1().min_h_0())
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(SECONDARY))
                    .child("Play your song in any app. Sing along here."),
            );
        if let Some(picker) = self.picker {
            content = content.child(self.popup(picker, cx));
        }
        content
    }
}
