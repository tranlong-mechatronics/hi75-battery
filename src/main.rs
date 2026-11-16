#![windows_subsystem = "windows"]

mod hid;
mod icon;
mod widget;

use auto_launch::AutoLaunch;
use hid::{ConnectionKind, KeyboardReader};
use icon::create_battery_icon;
use std::env;
use std::mem::zeroed;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIconBuilder,
};
use widget::{TaskbarWidget, WM_WIDGET_QUIT, WM_WIDGET_REFRESH, WM_WIDGET_TOGGLE_AUTOSTART};
use winapi::um::winuser::{
    DispatchMessageW, GetMessageW, GetSystemMetrics, OpenDesktopW, PostQuitMessage,
    PostThreadMessageW, SetProcessDpiAwarenessContext, SetThreadDesktop, TranslateMessage, MSG,
    SM_CXSMICON, WM_APP,
};

const WM_UPDATE_BATTERY: u32 = WM_APP + 10;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(
        lp_mutex_attributes: *mut winapi::ctypes::c_void,
        b_initial_owner: i32,
        lp_name: *const u16,
    ) -> *mut winapi::ctypes::c_void;
}

fn log(msg: &str) {
    if env::var_os("HI75_DEBUG").is_none() {
        return;
    }
    use std::io::Write;
    let Ok(exe) = env::current_exe() else { return };
    let log_path = exe.with_file_name("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(
            f,
            "[{}] {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            msg
        );
    }
}

fn format_tooltip(battery_opt: Option<u8>, is_charging: bool) -> String {
    match (battery_opt, is_charging) {
        (Some(pct), true) => format!("Leobog Hi75C Pro: {}% (Đang sạc)", pct),
        (Some(pct), false) => format!("Leobog Hi75C Pro: {}% (Đang dùng pin)", pct),
        (None, true) => "Leobog Hi75C Pro: Đang sạc qua dây USB".to_string(),
        (None, false) => "Leobog Hi75C Pro: Đang ngủ / Chưa kết nối".to_string(),
    }
}

fn ensure_installed_and_autostart() -> bool {
    let args: Vec<String> = env::args().collect();
    let explicit_install = args.iter().any(|a| a == "--install");
    let explicit_uninstall = args.iter().any(|a| a == "--uninstall");

    let Ok(current_exe) = env::current_exe() else {
        return false;
    };

    let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
        return false;
    };
    let install_dir = std::path::PathBuf::from(local_app_data).join("LeobogHi75Battery");
    let installed_exe = install_dir.join("Leobog-Hi75C-Pro-Battery.exe");

    if explicit_uninstall {
        if let Some(path_str) = installed_exe.to_str() {
            let auto = AutoLaunch::new(
                "Hi75BatteryMonitor",
                path_str,
                auto_launch::WindowsEnableMode::CurrentUser,
                &["--autostart"],
            );
            let _ = auto.disable();
        }
        return true;
    }

    // Never relocate the developer's own cargo target binary unless --install is explicitly passed
    let cur_str_lower = current_exe.to_string_lossy().to_lowercase();
    let is_dev_target = cur_str_lower.contains("\\target\\release\\")
        || cur_str_lower.contains("\\target\\debug\\");
    let already_in_install_dir = current_exe
        .parent()
        .map(|p| p.to_string_lossy().eq_ignore_ascii_case(&install_dir.to_string_lossy()))
        .unwrap_or(false);

    if explicit_install || (!is_dev_target && !already_in_install_dir) {
        let _ = std::fs::create_dir_all(&install_dir);
        if std::fs::copy(&current_exe, &installed_exe).is_ok() {
            if let Some(path_str) = installed_exe.to_str() {
                let auto = AutoLaunch::new(
                    "Hi75BatteryMonitor",
                    path_str,
                    auto_launch::WindowsEnableMode::CurrentUser,
                    &["--autostart"],
                );
                let _ = auto.enable();
            }
            let _ = std::process::Command::new(&installed_exe)
                .arg("--autostart")
                .spawn();
            return true;
        }
    }

    false
}

fn main() {
    if ensure_installed_and_autostart() {
        return;
    }

    // Đảm bảo chỉ chạy duy nhất 1 tiến trình (tránh mở trùng khi click nhiều lần)
    unsafe {
        let mutex_name: Vec<u16> = "Local\\LeobogHi75CBatteryMonitorMutex\0"
            .encode_utf16()
            .collect();
        let h_mutex = CreateMutexW(null_mut(), 1, mutex_name.as_ptr());
        if !h_mutex.is_null() && winapi::um::errhandlingapi::GetLastError() == 183 {
            // ERROR_ALREADY_EXISTS = 183
            return;
        }
    }

    log("hi75-battery starting...");

    std::panic::set_hook(Box::new(|info| {
        log(&format!("PANIC: {:?}", info));
    }));

    // 0. Đảm bảo thread kết nối tới interactive desktop "default" và bật PerMonitorV2 DPI awareness
    unsafe {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4isize
        SetProcessDpiAwarenessContext(-4isize as _);
        let default_name: Vec<u16> = "default\0".encode_utf16().collect();
        let target_desk = OpenDesktopW(default_name.as_ptr(), 0, 0, 0x10000000);
        log(&format!("OpenDesktopW: {:?}", target_desk));
        if !target_desk.is_null() {
            let res = SetThreadDesktop(target_desk);
            log(&format!("SetThreadDesktop: {}", res));
        }
    }

    log("Step 1: getting main_thread_id");
    let main_thread_id = unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() };

    log("Step 2: auto_launch");
    let current_exe = env::current_exe().expect("Lấy đường dẫn exe thất bại");
    let auto_launcher = AutoLaunch::new(
        "Hi75BatteryMonitor",
        current_exe.to_str().unwrap(),
        auto_launch::WindowsEnableMode::CurrentUser,
        &["--autostart"],
    );

    let is_auto_enabled = auto_launcher.is_enabled().unwrap_or(false);
    let autostart_enabled = Arc::new(AtomicBool::new(is_auto_enabled));

    log("Step 3: Creating TaskbarWidget...");
    let mut taskbar_widget = TaskbarWidget::new(main_thread_id, Arc::clone(&autostart_enabled));
    log(&format!("TaskbarWidget created: {}", taskbar_widget.is_some()));

    let mut tray_icon: Option<tray_icon::TrayIcon> = None;
    let mut refresh_id = None;
    let mut auto_launch_id = None;
    let mut quit_id = None;
    let mut auto_launch_item_opt: Option<CheckMenuItem> = None;

    if taskbar_widget.is_none() {
        log("Step 4: TaskbarWidget unavailable, falling back to TrayIcon...");
        let tray_menu = Menu::new();
        let refresh_item = MenuItem::new("Làm mới (Refresh)", true, None);
        let auto_launch_item =
            CheckMenuItem::new("Khởi động cùng Windows", true, is_auto_enabled, None);
        let quit_item = MenuItem::new("Thoát (Exit)", true, None);

        refresh_id = Some(refresh_item.id().clone());
        auto_launch_id = Some(auto_launch_item.id().clone());
        quit_id = Some(quit_item.id().clone());

        let _ = tray_menu.append(&refresh_item);
        let _ = tray_menu.append(&auto_launch_item);
        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let _ = tray_menu.append(&quit_item);
        auto_launch_item_opt = Some(auto_launch_item);

        let tray_icon_size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
        let raw_initial = create_battery_icon(None, false, tray_icon_size);
        let initial_icon = Icon::from_rgba(raw_initial, tray_icon_size, tray_icon_size)
            .expect("Khởi tạo Icon thất bại");

        tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("Leobog Hi75C Pro: Đang kết nối...")
            .with_icon(initial_icon)
            .build()
            .ok();
        log(&format!("Fallback TrayIcon built: {}", tray_icon.is_some()));
    }

    let (tx_refresh, rx_refresh) = channel::<()>();
    let (tx_battery, rx_battery) = channel::<(Option<u8>, bool)>();

    let is_running = Arc::new(AtomicBool::new(true));
    let is_running_worker = Arc::clone(&is_running);

    // 4. Background Worker định kỳ lấy % pin từ phím (tự chuyển đổi giữa USB-C dây & 2.4G)
    thread::spawn(move || {
        let mut reader: Option<KeyboardReader> = None;
        let mut misses = 0;

        while is_running_worker.load(Ordering::SeqCst) {
            if reader.is_none() {
                reader = KeyboardReader::open();
                log(&format!(
                    "Worker: KeyboardReader::open() = {:?}",
                    reader.as_ref().map(|r| r.kind())
                ));
            } else if let Some(r) = &reader {
                if r.kind() == ConnectionKind::Wireless && KeyboardReader::is_wired_present() {
                    log("Worker: Wired USB detected, switching from Wireless to Wired...");
                    reader = KeyboardReader::open();
                    misses = 0;
                }
            }

            if let Some(r) = &reader {
                match r.query_battery() {
                    Some(reading) => {
                        log(&format!(
                            "Worker: query_battery OK ({:?}): {:?}%, charging={}",
                            reading.mode, reading.percentage, reading.charging
                        ));
                        misses = 0;
                        let _ = tx_battery.send((reading.percentage, reading.charging));
                        unsafe {
                            PostThreadMessageW(main_thread_id, WM_UPDATE_BATTERY, 0, 0);
                        }
                    }
                    None => {
                        let was_wired = r.kind() == ConnectionKind::Wired;
                        misses += 1;
                        log(&format!("Worker: query_battery None (misses: {})", misses));
                        if was_wired {
                            // Cable unplugged: drop wired handle and immediately check 2.4G dongle
                            reader = KeyboardReader::open();
                            if let Some(new_r) = &reader {
                                if let Some(reading) = new_r.query_battery() {
                                    misses = 0;
                                    let _ = tx_battery.send((reading.percentage, reading.charging));
                                    unsafe {
                                        PostThreadMessageW(main_thread_id, WM_UPDATE_BATTERY, 0, 0);
                                    }
                                    let _ = rx_refresh.recv_timeout(Duration::from_secs(30));
                                    continue;
                                }
                            }
                        }
                        if misses >= 4 || reader.is_none() {
                            reader = None;
                            let _ = tx_battery.send((None, false));
                            unsafe {
                                PostThreadMessageW(main_thread_id, WM_UPDATE_BATTERY, 0, 0);
                            }
                        }
                    }
                }
            } else {
                log("Worker: reader is None");
                let _ = tx_battery.send((None, false));
                unsafe {
                    PostThreadMessageW(main_thread_id, WM_UPDATE_BATTERY, 0, 0);
                }
            }

            // Polling chu kỳ 30 giây hoặc thức dậy ngay nếu người dùng ấn Làm mới
            let _ = rx_refresh.recv_timeout(Duration::from_secs(30));
        }
    });

    let (tx_menu, rx_menu) = channel::<MenuEvent>();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = tx_menu.send(event);
        unsafe {
            PostThreadMessageW(main_thread_id, WM_UPDATE_BATTERY, 0, 0);
        }
    }));

    let toggle_autostart = || {
        let current = auto_launcher.is_enabled().unwrap_or(false);
        let new_state = !current;
        if new_state {
            let _ = auto_launcher.enable();
        } else {
            let _ = auto_launcher.disable();
        }
        autostart_enabled.store(new_state, Ordering::SeqCst);
        if let Some(ref item) = auto_launch_item_opt {
            item.set_checked(new_state);
        }
    };

    log("Entering GetMessageW loop...");
    unsafe {
        let mut msg: MSG = zeroed();

        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() {
                match msg.message {
                    WM_WIDGET_REFRESH => {
                        let _ = tx_refresh.send(());
                    }
                    WM_WIDGET_TOGGLE_AUTOSTART => {
                        toggle_autostart();
                    }
                    WM_WIDGET_QUIT => {
                        log("User clicked Quit from widget, posting WM_QUIT");
                        PostQuitMessage(0);
                    }
                    _ => {}
                }
            }

            // Xử lý sự kiện click menu chuột phải trên fallback tray icon (nếu có)
            while let Ok(event) = rx_menu.try_recv() {
                if auto_launch_id.as_ref() == Some(&event.id) {
                    toggle_autostart();
                } else if refresh_id.as_ref() == Some(&event.id) {
                    let _ = tx_refresh.send(());
                } else if quit_id.as_ref() == Some(&event.id) {
                    log("User clicked Quit, posting WM_QUIT");
                    PostQuitMessage(0);
                    break;
                }
            }

            // Xử lý cập nhật pin từ background worker
            while let Ok((battery_opt, is_charging)) = rx_battery.try_recv() {
                log(&format!(
                    "Main: Updating indicator, battery={:?}, charging={}",
                    battery_opt, is_charging
                ));
                let tooltip = format_tooltip(battery_opt, is_charging);

                if let Some(ref tray) = tray_icon {
                    let icon_sz = GetSystemMetrics(SM_CXSMICON).max(16) as u32;
                    let raw_icon = create_battery_icon(battery_opt, is_charging, icon_sz);
                    if let Ok(icon) = Icon::from_rgba(raw_icon, icon_sz, icon_sz) {
                        let set_res = tray.set_icon(Some(icon));
                        let tip_res = tray.set_tooltip(Some(tooltip.clone()));
                        log(&format!("Main: set_icon={:?}, set_tooltip={:?}", set_res, tip_res));
                    }
                }

                if let Some(ref mut w) = taskbar_widget {
                    w.update(battery_opt, is_charging, &tooltip);
                }
            }

            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        log("Exited GetMessageW loop!");
    }

    drop(taskbar_widget);
    is_running.store(false, Ordering::SeqCst);
    log("hi75-battery finished cleanly.");
}
