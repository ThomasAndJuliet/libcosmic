// SPDX-License-Identifier: MPL-2.0

//! Quitting an application must not crash, and must not leave the SCTK thread
//! (`ct-wayland`) behind.
//!
//! iced_winit runs a second, foreign wayland-client connection on winit's
//! `wl_display` in a helper thread. When that thread was detached, it
//! destroyed its proxies while the main thread dropped winit's event loop,
//! which disconnects and frees the display. When the disconnect won, the
//! process died with a segfault in libwayland at exit. The fix stops and
//! joins the thread before the event loop is dropped.
//!
//! winit allows one event loop per process, so this test runs itself again.
//!
//! * Parent (no `EXIT_TEARDOWN_CHILD`): starts its own headless weston, then
//!   runs the child `EXIT_TEARDOWN_RUNS` times (20) for each variant
//!   (`EXIT_TEARDOWN_VARIANTS`, default `plain,popup,window`). It asserts
//!   that the child exits with code 0 (not killed by a signal, no panic) and
//!   logs no "did not stop". No weston means the test fails: it never skips.
//! * Child (`EXIT_TEARDOWN_CHILD=plain|popup|window`): a minimal COSMIC
//!   application quits by itself: with `iced::exit()` from its main window,
//!   with a popup open, or as COSMIC Terminal quits when its command ends
//!   (no main window; a window opened with `window::open` is closed as soon
//!   as it opens, and the application exits when it is gone). When `run`
//!   has returned, the display is gone, so any thread still named
//!   `ct-wayland` is a thread nobody joined: exit code 3.
//!
//! The `window` variant quits while iced_winit is still handling the
//! window's creation: the instance finishes inside a nested
//! `Runner::process_event`, and polling it again afterwards panicked with
//! "`async fn` resumed after completion".
//!
//! Set `EXIT_TEARDOWN_COMPOSITOR=external` to use the compositor in
//! `WAYLAND_DISPLAY` instead of starting weston.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cosmic::app::{Core, Settings, Task};
use cosmic::iced::Subscription;
use cosmic::iced::platform_specific::runtime::wayland::popup::{SctkPopupSettings, SctkPositioner};
use cosmic::cctk::wayland_protocols::xdg::shell::client::xdg_positioner::{Anchor, Gravity};
use cosmic::iced::{Rectangle, window};
use cosmic::surface::action::{LiveSettings, simple_popup};
use cosmic::{Application, Element};

const CHILD_ENV: &str = "EXIT_TEARDOWN_CHILD";

const EXIT_THREAD_LEFT: i32 = 3;
const EXIT_NO_POPUP: i32 = 4;
const EXIT_RUN_FAILED: i32 = 5;

/// Set by the popup's view function, so a popup that never got built fails
/// the popup variant instead of passing without testing anything.
static POPUP_DRAWN: AtomicBool = AtomicBool::new(false);

fn main() {
    match std::env::var(CHILD_ENV).as_deref() {
        Ok("plain") => child(Variant::Plain),
        Ok("popup") => child(Variant::Popup),
        Ok("window") => child(Variant::Window),
        Ok(other) => panic!("unknown {CHILD_ENV} value {other:?}"),
        Err(_) => parent(),
    }
}

// ---------------------------------------------------------------------------
// Child: a minimal application that quits by itself.

#[derive(Clone, Debug)]
enum Message {
    Tick,
    Opened(window::Id),
    Closed,
}

/// How the child quits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// `iced::exit()` from the main window.
    Plain,
    /// `iced::exit()` with a popup open.
    Popup,
    /// As COSMIC Terminal quits when its command ends (`-- true`): no main
    /// window; one window opened with `window::open`, closed as soon as it
    /// has opened, and `iced::exit()` when it is gone.
    Window,
}

struct App {
    core: Core,
    ticks: u32,
    variant: Variant,
}

impl Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Variant;
    type Message = Message;
    const APP_ID: &'static str = "org.cosmic.ExitTeardownTest";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, variant: Variant) -> (Self, Task<Message>) {
        let task = if variant == Variant::Window {
            let (_, opened) = window::open(window::Settings {
                exit_on_close_request: false,
                ..window::Settings::default()
            });
            opened.map(|id| cosmic::Action::App(Message::Opened(id)))
        } else {
            Task::none()
        };
        (
            App {
                core,
                ticks: 0,
                variant,
            },
            task,
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.variant == Variant::Window {
            return Subscription::none();
        }
        cosmic::iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }

    fn on_close_requested(&self, _id: window::Id) -> Option<Message> {
        (self.variant == Variant::Window).then_some(Message::Closed)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Opened(id) => window::close(id),
            Message::Closed => cosmic::iced::exit(),
            Message::Tick => {
                let with_popup = self.variant == Variant::Popup;
                self.ticks += 1;
                // The popup variant gets more time to be built and shown.
                let quit_at = if with_popup { 8 } else { 3 };
                if self.ticks >= quit_at {
                    return cosmic::iced::exit();
                }
                // The SCTK thread must know the parent window, so the popup
                // is requested after start-up, not in `init`.
                if with_popup && self.ticks == 2 {
                    return self.open_popup();
                }
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        cosmic::widget::text("exit teardown test").into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        cosmic::widget::text("exit teardown test window").into()
    }
}

impl App {
    /// An xdg popup on the main window, with no grab. The SCTK thread owns
    /// its `xdg_popup` and `wl_surface` when the application quits.
    fn open_popup(&self) -> Task<Message> {
        let parent = self.core.main_window_id().expect("a main window");
        let popup = window::Id::unique();
        let action = simple_popup::<Message>(
            LiveSettings::default,
            move || SctkPopupSettings {
                parent,
                id: popup,
                input_zone: None,
                positioner: SctkPositioner {
                    size: Some((120, 60)),
                    anchor_rect: Rectangle {
                        x: 10,
                        y: 10,
                        width: 40,
                        height: 20,
                    },
                    anchor: Anchor::BottomLeft,
                    gravity: Gravity::BottomRight,
                    ..Default::default()
                },
                parent_size: None,
                grab: false,
                close_with_children: true,
            },
            Some(|| -> Element<'static, cosmic::Action<Message>> {
                POPUP_DRAWN.store(true, Ordering::SeqCst);
                cosmic::widget::text("popup").into()
            }),
        );
        cosmic::task::message(cosmic::Action::Surface(action))
    }
}

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {}
}

fn sctk_threads() -> Vec<String> {
    std::fs::read_dir("/proc/self/task")
        .expect("read /proc/self/task")
        .filter_map(|entry| {
            let comm = std::fs::read_to_string(entry.ok()?.path().join("comm")).ok()?;
            let comm = comm.trim().to_string();
            (comm == "ct-wayland").then_some(comm)
        })
        .collect()
}

fn child(variant: Variant) -> ! {
    static LOGGER: StderrLogger = StderrLogger;
    log::set_logger(&LOGGER).expect("set logger");
    log::set_max_level(log::LevelFilter::Warn);

    let settings = match variant {
        Variant::Window => Settings::default().no_main_window(true).exit_on_close(false),
        Variant::Plain | Variant::Popup => Settings::default(),
    };
    let result = cosmic::app::run::<App>(settings, variant);
    // `run` has returned: winit's event loop, and the display, are gone.
    if let Err(err) = result {
        eprintln!("run failed: {err}");
        std::process::exit(EXIT_RUN_FAILED);
    }
    let threads = sctk_threads();
    if !threads.is_empty() {
        eprintln!("{} ct-wayland thread(s) still alive after run returned", threads.len());
        std::process::exit(EXIT_THREAD_LEFT);
    }
    if variant == Variant::Popup && !POPUP_DRAWN.load(Ordering::SeqCst) {
        eprintln!("the popup was never drawn, so this run tested nothing");
        std::process::exit(EXIT_NO_POPUP);
    }
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// Parent: a headless compositor, then the children.

use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

struct Weston {
    child: Child,
    log: PathBuf,
}

impl Drop for Weston {
    fn drop(&mut self) {
        _ = self.child.kill();
        _ = self.child.wait();
    }
}

fn start_weston(runtime_dir: &Path, socket: &str) -> Weston {
    let log = runtime_dir.join("weston.log");
    let log_file = std::fs::File::create(&log).expect("create the weston log");
    let child = Command::new("weston")
        .args(["--backend=headless", "--width=1024", "--height=768", "--idle-time=0"])
        .arg(format!("--socket={socket}"))
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(log_file.try_clone().unwrap())
        .stderr(log_file)
        .spawn()
        .unwrap_or_else(|err| {
            panic!(
                "this test needs a headless compositor, and `weston` could not be started \
                 ({err}). Install weston (apt-get install weston), or set \
                 EXIT_TEARDOWN_COMPOSITOR=external and WAYLAND_DISPLAY to use another \
                 compositor. The test does not skip."
            )
        });
    let mut weston = Weston { child, log };
    let socket_path = runtime_dir.join(socket);
    for _ in 0..200 {
        if socket_path.exists() {
            return weston;
        }
        if let Some(status) = weston.child.try_wait().unwrap() {
            panic!(
                "weston exited early ({status}); its log:\n{}",
                std::fs::read_to_string(&weston.log).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "weston did not create {} within 10 s; its log:\n{}",
        socket_path.display(),
        std::fs::read_to_string(&weston.log).unwrap_or_default()
    );
}

fn run_child(variant: &str, env: &[(String, String)], timeout: Duration) -> (String, std::process::ExitStatus) {
    let exe = std::env::current_exe().expect("current_exe");
    let mut command = Command::new(exe);
    command
        .env(CHILD_ENV, variant)
        .env_remove("DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("start the child");
    let mut stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut text).ok();
        text
    });
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > timeout {
            _ = child.kill();
            child.wait().unwrap();
            let text = reader.join().unwrap();
            panic!("the {variant} child did not exit within {timeout:?}; stderr:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (reader.join().unwrap(), status)
}

fn parent() {
    let runs: usize = std::env::var("EXIT_TEARDOWN_RUNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);

    let scratch = tempfile::tempdir().expect("temporary directory");
    let runtime_dir = scratch.path().join("runtime");
    std::fs::create_dir(&runtime_dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let external = std::env::var("EXIT_TEARDOWN_COMPOSITOR").as_deref() == Ok("external");
    let socket = "wayland-exit-teardown";
    let (weston, mut env): (Option<Weston>, Vec<(String, String)>) = if external {
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| {
            panic!("EXIT_TEARDOWN_COMPOSITOR=external needs WAYLAND_DISPLAY; the test does not skip")
        });
        let dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
        (
            None,
            vec![
                ("WAYLAND_DISPLAY".into(), display),
                ("XDG_RUNTIME_DIR".into(), dir),
            ],
        )
    } else {
        (
            Some(start_weston(&runtime_dir, socket)),
            vec![
                ("WAYLAND_DISPLAY".into(), socket.into()),
                ("XDG_RUNTIME_DIR".into(), runtime_dir.display().to_string()),
            ],
        )
    };
    // Nothing the children do may touch the real home folder.
    for (name, dir) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        let path = scratch.path().join(dir);
        std::fs::create_dir(&path).unwrap();
        env.push((name.into(), path.display().to_string()));
    }

    let mut failures = Vec::new();
    let mut passed = 0;
    // EXIT_TEARDOWN_VARIANTS=window,plain runs only those.
    let variants: Vec<String> = std::env::var("EXIT_TEARDOWN_VARIANTS")
        .map(|list| list.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|_| ["plain", "popup", "window"].map(str::to_owned).to_vec());
    for variant in variants.iter().map(String::as_str) {
        let mut ok = 0;
        for run in 1..=runs {
            let (stderr, status) = run_child(variant, &env, Duration::from_secs(60));
            let mut problems = Vec::new();
            if let Some(signal) = status.signal() {
                problems.push(format!("killed by signal {signal}"));
            } else if status.code() != Some(0) {
                problems.push(format!(
                    "exit code {:?} ({})",
                    status.code(),
                    match status.code() {
                        Some(EXIT_THREAD_LEFT) => "a ct-wayland thread outlived run",
                        Some(EXIT_NO_POPUP) => "the popup was never drawn",
                        Some(EXIT_RUN_FAILED) => "run failed",
                        Some(101) => "a panic",
                        _ => "unexpected",
                    }
                ));
            }
            if stderr.contains("did not stop") {
                problems.push("logged \"did not stop\" (the join timed out)".into());
            }
            if problems.is_empty() {
                ok += 1;
            } else {
                failures.push(format!(
                    "{variant} run {run}: {}\n--- child stderr ---\n{}",
                    problems.join("; "),
                    stderr.trim_end()
                ));
            }
        }
        println!(
            "test exit_teardown::{variant} ... {} ({ok}/{runs} clean exits)",
            if ok == runs { "ok" } else { "FAILED" }
        );
        passed += usize::from(ok == runs);
    }
    drop(weston);

    if !failures.is_empty() {
        eprintln!("\n{} failing run(s):\n", failures.len());
        for failure in &failures {
            eprintln!("{failure}\n");
        }
        println!("\ntest result: FAILED. {passed} passed; {} failed", variants.len() - passed);
        panic!("quitting an application was not clean in {} run(s)", failures.len());
    }
    println!("\ntest result: ok. {passed} passed; 0 failed");
}
