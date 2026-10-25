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
//!   (`EXIT_TEARDOWN_VARIANTS`, default `plain,popup,window,lost`). It
//!   asserts that the child exits with code 0 (not killed by a signal, no
//!   panic) and logs no "did not stop". No weston means the test fails: it
//!   never skips.
//! * Child (`EXIT_TEARDOWN_CHILD=plain|popup|window|lost`): a minimal COSMIC
//!   application quits by itself: with `iced::exit()` from its main window,
//!   with a popup open, or as COSMIC Terminal quits when its command ends
//!   (no main window; a window opened with `window::open` is closed as soon
//!   as it opens, and the application exits when it is gone). When `run`
//!   has returned, the display is gone, so any thread still named
//!   `ct-wayland` is a thread nobody joined: exit code 3.
//!
//! The `lost` variant does not quit: the compositor goes away under it, as
//! when the compositor crashes or the session ends. Each run has its own
//! weston, which the parent kills (SIGKILL) once the child is up. `run` may
//! then return an error; the child must still end within
//! `LOST_EXIT_BOUND` of the kill, with the SCTK thread stopped and joined.
//! The SCTK thread may report the dead connection in at most
//! `LOST_MAX_SCTK_ERROR_LINES` lines, and the whole process in at most
//! `LOST_MAX_ERROR_LINES` (other parts of the stack print a few lines of
//! their own). The SCTK thread used to dispatch the dead connection in a busy
//! loop, logging "Broken pipe" tens of thousands of times a second and never
//! reaching `Action::Shutdown`, so the join timed out.
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
const EXIT_NO_SCTK_THREAD: i32 = 6;

/// `lost`: the child writes this file when it is up, and the parent then
/// kills the compositor.
const READY_ENV: &str = "EXIT_TEARDOWN_READY";
/// `lost`: how long the child may take to end once the compositor is killed.
const LOST_EXIT_BOUND: Duration = Duration::from_secs(10);
/// `lost`: how many stderr lines from the SCTK thread may report the dead
/// connection.
const LOST_MAX_SCTK_ERROR_LINES: usize = 2;
/// `lost`: how many stderr lines in all may report the dead connection.
const LOST_MAX_ERROR_LINES: usize = 20;
/// The log target of the SCTK thread's loop.
const SCTK_TARGET: &str = "iced_winit::platform_specific::wayland::event_loop:";

/// Set by the popup's view function, so a popup that never got built fails
/// the popup variant instead of passing without testing anything.
static POPUP_DRAWN: AtomicBool = AtomicBool::new(false);

fn main() {
    match std::env::var(CHILD_ENV).as_deref() {
        Ok("plain") => child(Variant::Plain),
        Ok("popup") => child(Variant::Popup),
        Ok("window") => child(Variant::Window),
        Ok("lost") => child(Variant::Lost),
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
    /// Does not quit: the compositor is killed under it.
    Lost,
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
                if self.variant == Variant::Lost {
                    if self.ticks == 3 {
                        signal_ready();
                    }
                    return Task::none();
                }
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

/// `lost`: tells the parent that the application is up, with its SCTK
/// thread running, so that the compositor can be killed.
fn signal_ready() {
    if sctk_threads().is_empty() {
        eprintln!("no ct-wayland thread is running, so this run would test nothing");
        std::process::exit(EXIT_NO_SCTK_THREAD);
    }
    let path = std::env::var(READY_ENV).expect("EXIT_TEARDOWN_READY");
    std::fs::write(path, b"ready").expect("write the ready file");
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
        Variant::Plain | Variant::Popup | Variant::Lost => Settings::default(),
    };
    let result = cosmic::app::run::<App>(settings, variant);
    // `run` has returned: winit's event loop, and the display, are gone.
    match result {
        // With the compositor gone, an error is the expected way to end.
        Err(err) if variant == Variant::Lost => eprintln!("run ended with: {err}"),
        Err(err) => {
            eprintln!("run failed: {err}");
            std::process::exit(EXIT_RUN_FAILED);
        }
        Ok(()) => {}
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

fn spawn_child(variant: &str, env: &[(String, String)]) -> (Child, std::thread::JoinHandle<String>) {
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
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut stderr, &mut bytes).ok();
        String::from_utf8_lossy(&bytes).into_owned()
    });
    (child, reader)
}

/// Waits for the child to exit, or kills it after `timeout`: `None`.
fn wait_child(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if start.elapsed() > timeout {
            _ = child.kill();
            child.wait().unwrap();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn run_child(variant: &str, env: &[(String, String)], timeout: Duration) -> (String, std::process::ExitStatus) {
    let (mut child, reader) = spawn_child(variant, env);
    let status = wait_child(&mut child, timeout);
    let text = reader.join().unwrap();
    match status {
        Some(status) => (text, status),
        None => panic!("the {variant} child did not exit within {timeout:?}; stderr:\n{text}"),
    }
}

/// What a `lost` run saw.
struct LostRun {
    stderr: String,
    /// `None`: still running `LOST_EXIT_BOUND` after the kill (then killed).
    status: Option<std::process::ExitStatus>,
    /// From killing the compositor to the child's exit.
    took: Duration,
}

/// One `lost` run: its own weston, killed once the child is up.
fn run_lost_child(scratch: &Path, run: usize, base_env: &[(String, String)]) -> LostRun {
    let runtime_dir = scratch.join(format!("lost-runtime-{run}"));
    std::fs::create_dir(&runtime_dir).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let socket = "wayland-exit-teardown-lost";
    let mut weston = start_weston(&runtime_dir, socket);
    let ready = runtime_dir.join("ready");
    let mut env: Vec<(String, String)> = base_env
        .iter()
        .filter(|(key, _)| key != "WAYLAND_DISPLAY" && key != "XDG_RUNTIME_DIR")
        .cloned()
        .collect();
    env.push(("WAYLAND_DISPLAY".into(), socket.into()));
    env.push(("XDG_RUNTIME_DIR".into(), runtime_dir.display().to_string()));
    env.push((READY_ENV.into(), ready.display().to_string()));

    let (mut child, reader) = spawn_child("lost", &env);
    let start = std::time::Instant::now();
    while !ready.exists() {
        if let Some(status) = child.try_wait().unwrap() {
            let text = reader.join().unwrap();
            panic!("the lost child exited ({status}) before it was up; stderr:\n{text}");
        }
        if start.elapsed() > Duration::from_secs(60) {
            _ = child.kill();
            child.wait().unwrap();
            let text = reader.join().unwrap();
            panic!("the lost child was not up within 60 s; stderr:\n{text}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    // The compositor goes away: SIGKILL, as in a crash.
    _ = weston.child.kill();
    _ = weston.child.wait();
    let killed = std::time::Instant::now();
    let status = wait_child(&mut child, LOST_EXIT_BOUND);
    let took = killed.elapsed();
    LostRun {
        stderr: reader.join().unwrap(),
        status,
        took,
    }
}

/// Lines that report the dead connection: in all, and from the SCTK thread.
fn connection_error_lines(stderr: &str) -> (usize, usize) {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| {
            line.contains("Broken pipe")
                || line.contains("Connection reset")
                || line.contains("dispatch error")
                || line.contains("connection lost")
                || line.contains("flush the wayland display")
                || line.contains("Protocol error")
        })
        .collect();
    let sctk = lines.iter().filter(|line| line.contains(SCTK_TARGET)).count();
    (lines.len(), sctk)
}

/// The first and last few lines of a possibly huge stderr.
fn abridged(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.trim_end().lines().collect();
    if lines.len() <= 40 {
        return lines.join("\n");
    }
    format!(
        "{}\n[... {} lines left out ...]\n{}",
        lines[..20].join("\n"),
        lines.len() - 40,
        lines[lines.len() - 20..].join("\n")
    )
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
        .unwrap_or_else(|_| ["plain", "popup", "window", "lost"].map(str::to_owned).to_vec());
    for variant in variants.iter().map(String::as_str) {
        let mut ok = 0;
        // `lost`: the slowest exit after the kill, and the most error lines.
        let mut slowest = Duration::ZERO;
        let mut most_errors = 0;
        let mut most_sctk_errors = 0;
        for run in 1..=runs {
            let mut problems = Vec::new();
            let (stderr, status) = if variant == "lost" {
                let lost = run_lost_child(scratch.path(), run, &env);
                slowest = slowest.max(lost.took);
                let (errors, sctk_errors) = connection_error_lines(&lost.stderr);
                most_errors = most_errors.max(errors);
                most_sctk_errors = most_sctk_errors.max(sctk_errors);
                if sctk_errors > LOST_MAX_SCTK_ERROR_LINES {
                    problems.push(format!(
                        "{sctk_errors} lines from the SCTK thread report the dead connection \
                         (at most {LOST_MAX_SCTK_ERROR_LINES})"
                    ));
                }
                if errors > LOST_MAX_ERROR_LINES {
                    problems.push(format!(
                        "{errors} lines in all report the dead connection (at most {LOST_MAX_ERROR_LINES})"
                    ));
                }
                match lost.status {
                    Some(status) => (lost.stderr, status),
                    None => {
                        problems.push(format!(
                            "still running {LOST_EXIT_BOUND:?} after the compositor was killed"
                        ));
                        failures.push(format!(
                            "{variant} run {run}: {}\n--- child stderr ---\n{}",
                            problems.join("; "),
                            abridged(&lost.stderr)
                        ));
                        continue;
                    }
                }
            } else {
                run_child(variant, &env, Duration::from_secs(60))
            };
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
                        Some(EXIT_NO_SCTK_THREAD) => "no SCTK thread was running",
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
                    abridged(&stderr)
                ));
            }
        }
        let lost_detail = if variant == "lost" {
            format!(
                "; slowest exit {slowest:.2?} after the kill; at most {most_sctk_errors} error \
                 lines from the SCTK thread, {most_errors} in all"
            )
        } else {
            String::new()
        };
        println!(
            "test exit_teardown::{variant} ... {} ({ok}/{runs} clean exits{lost_detail})",
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
