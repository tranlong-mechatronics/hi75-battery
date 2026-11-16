use crate::icon::{create_taskbar_widget_bgra, is_light_theme, WIDGET_LOGICAL_HEIGHT};
use winapi::ctypes::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winapi::shared::minwindef::{DWORD, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HWND, POINT, RECT, SIZE};
use winapi::um::commctrl::{
    InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_BAR_CLASSES, TOOLINFOW, TTF_SUBCLASS,
    TTM_ADDTOOLW, TTM_NEWTOOLRECTW, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX,
};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use winapi::um::winuser::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    FindWindowExW, FindWindowW, GetClassNameW, GetCursorPos, GetDC, GetDpiForWindow,
    GetForegroundWindow, GetMonitorInfoW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    IsWindowVisible, KillTimer, LoadCursorW, MonitorFromWindow, PostThreadMessageW,
    RegisterClassExW, ReleaseDC, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TrackMouseEvent, TrackPopupMenu, UpdateLayeredWindow, CREATESTRUCTW, CW_USEDEFAULT,
    GWLP_USERDATA, GW_HWNDPREV, HWND_TOPMOST, IDC_ARROW, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MONITORINFO, MONITOR_DEFAULTTONEAREST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, TME_LEAVE, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TRACKMOUSEEVENT, ULW_ALPHA, WM_APP, WM_DPICHANGED,
    WM_LBUTTONUP, WM_MOUSELEAVE, WM_MOUSEMOVE, WM_NCCREATE, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

pub const WM_WIDGET_REFRESH: u32 = WM_APP + 20;
pub const WM_WIDGET_TOGGLE_AUTOSTART: u32 = WM_APP + 21;
pub const WM_WIDGET_QUIT: u32 = WM_APP + 22;

const TIMER_SYNC_ID: usize = 1;
const CMD_REFRESH: usize = 1001;
const CMD_AUTOSTART: usize = 1002;
const CMD_QUIT: usize = 1003;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct WidgetState {
    hwnd: HWND,
    tooltip_hwnd: HWND,
    main_thread_id: u32,
    autostart_enabled: Arc<AtomicBool>,
    battery: Option<u8>,
    charging: bool,
    hovered: bool,
    visible: bool,
    scale: f32,
    width_px: i32,
    height_px: i32,
    pos_x: i32,
    pos_y: i32,
    last_light_theme: bool,
    tooltip_text: String,
}

pub struct TaskbarWidget {
    hwnd: HWND,
    state_ptr: *mut WidgetState,
}

impl TaskbarWidget {
    pub fn new(main_thread_id: u32, autostart_enabled: Arc<AtomicBool>) -> Option<Self> {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as DWORD,
                dwICC: ICC_BAR_CLASSES,
            };
            InitCommonControlsEx(&icc);

            let hinstance = GetModuleHandleW(null_mut());
            let class_name = to_wide("Hi75TaskbarWidgetClass");
            let wnd_title = to_wide("Hi75 Battery Widget");

            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as UINT,
                style: 0,
                lpfnWndProc: Some(widget_wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: null_mut(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: null_mut(),
                lpszMenuName: null_mut(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: null_mut(),
            };
            RegisterClassExW(&wc);

            let initial_scale = detect_taskbar_scale();
            let initial_w = (56.0 * initial_scale).round() as i32;
            let initial_h = (WIDGET_LOGICAL_HEIGHT * initial_scale).round() as i32;
            let tooltip_text = "Leobog Hi75C Pro: Đang kết nối...".to_string();

            let state = Box::new(WidgetState {
                hwnd: null_mut(),
                tooltip_hwnd: null_mut(),
                main_thread_id,
                autostart_enabled,
                battery: None,
                charging: false,
                hovered: false,
                visible: false,
                scale: initial_scale,
                width_px: initial_w,
                height_px: initial_h,
                pos_x: 0,
                pos_y: 0,
                last_light_theme: is_light_theme(),
                tooltip_text,
            });
            let state_ptr = Box::into_raw(state);

            let tray_cls = to_wide("Shell_TrayWnd");
            let shell_tray = FindWindowW(tray_cls.as_ptr(), null_mut());

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                wnd_title.as_ptr(),
                WS_POPUP,
                0,
                0,
                initial_w,
                initial_h,
                shell_tray,
                null_mut(),
                hinstance,
                state_ptr as *mut c_void,
            );

            if hwnd.is_null() {
                drop(Box::from_raw(state_ptr));
                return None;
            }

            (*state_ptr).hwnd = hwnd;

            // Create native Win32 tooltip bound to the widget client rect
            let tt_class = to_wide("tooltips_class32");
            let tt_hwnd = CreateWindowExW(
                WS_EX_TOPMOST,
                tt_class.as_ptr(),
                null_mut(),
                WS_POPUP | TTS_ALWAYSTIP | TTS_NOPREFIX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                hwnd,
                null_mut(),
                hinstance,
                null_mut(),
            );
            if !tt_hwnd.is_null() {
                (*state_ptr).tooltip_hwnd = tt_hwnd;
                let mut wide_tip = to_wide(&(*state_ptr).tooltip_text);
                let mut ti: TOOLINFOW = zeroed();
                ti.cbSize = size_of::<TOOLINFOW>() as UINT;
                ti.uFlags = TTF_SUBCLASS;
                ti.hwnd = hwnd;
                ti.uId = 1;
                ti.rect = RECT {
                    left: 0,
                    top: 0,
                    right: initial_w,
                    bottom: initial_h,
                };
                ti.lpszText = wide_tip.as_mut_ptr();
                winapi::um::winuser::SendMessageW(
                    tt_hwnd,
                    TTM_ADDTOOLW,
                    0,
                    &mut ti as *mut TOOLINFOW as LPARAM,
                );
            }

            render_widget(&mut *state_ptr);
            sync_position_and_visibility(&mut *state_ptr, true);
            SetTimer(hwnd, TIMER_SYNC_ID, 100, None);

            Some(Self { hwnd, state_ptr })
        }
    }

    pub fn update(&mut self, battery: Option<u8>, charging: bool, tooltip: &str) {
        unsafe {
            let state = &mut *self.state_ptr;
            let light_now = is_light_theme();
            let visual_changed = state.battery != battery
                || state.charging != charging
                || state.last_light_theme != light_now;
            let tip_changed = state.tooltip_text != tooltip;

            state.battery = battery;
            state.charging = charging;
            state.last_light_theme = light_now;

            if tip_changed {
                state.tooltip_text = tooltip.to_string();
                update_tooltip_text(state);
            }

            if visual_changed {
                let old_w = state.width_px;
                render_widget(state);
                if state.width_px != old_w {
                    update_tooltip_rect(state);
                }
                sync_position_and_visibility(state, state.width_px != old_w);
            }
        }
    }
}

impl Drop for TaskbarWidget {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_null() {
                KillTimer(self.hwnd, TIMER_SYNC_ID);
                DestroyWindow(self.hwnd);
            }
            if !self.state_ptr.is_null() {
                drop(Box::from_raw(self.state_ptr));
                self.state_ptr = null_mut();
            }
        }
    }
}

unsafe fn detect_taskbar_scale() -> f32 {
    let cls = to_wide("Shell_TrayWnd");
    let shell_tray = unsafe { FindWindowW(cls.as_ptr(), null_mut()) };
    if !shell_tray.is_null() {
        let dpi = unsafe { GetDpiForWindow(shell_tray) };
        if dpi > 0 {
            return dpi as f32 / 96.0;
        }
    }
    1.0
}

unsafe fn update_tooltip_text(state: &WidgetState) {
    if state.tooltip_hwnd.is_null() {
        return;
    }
    let mut wide_tip = to_wide(&state.tooltip_text);
    let mut ti: TOOLINFOW = unsafe { zeroed() };
    ti.cbSize = size_of::<TOOLINFOW>() as UINT;
    ti.hwnd = state.hwnd;
    ti.uId = 1;
    ti.lpszText = wide_tip.as_mut_ptr();
    unsafe {
        winapi::um::winuser::SendMessageW(
            state.tooltip_hwnd,
            TTM_UPDATETIPTEXTW,
            0,
            &mut ti as *mut TOOLINFOW as LPARAM,
        );
    }
}

unsafe fn update_tooltip_rect(state: &WidgetState) {
    if state.tooltip_hwnd.is_null() {
        return;
    }
    let mut ti: TOOLINFOW = unsafe { zeroed() };
    ti.cbSize = size_of::<TOOLINFOW>() as UINT;
    ti.hwnd = state.hwnd;
    ti.uId = 1;
    ti.rect = RECT {
        left: 0,
        top: 0,
        right: state.width_px,
        bottom: state.height_px,
    };
    unsafe {
        winapi::um::winuser::SendMessageW(
            state.tooltip_hwnd,
            TTM_NEWTOOLRECTW,
            0,
            &mut ti as *mut TOOLINFOW as LPARAM,
        );
    }
}

unsafe fn render_widget(state: &mut WidgetState) {
    let (w, h, bgra) =
        create_taskbar_widget_bgra(state.battery, state.charging, state.hovered, state.scale);
    state.width_px = w as i32;
    state.height_px = h as i32;

    unsafe {
        let screen_dc = GetDC(null_mut());
        let mem_dc = CreateCompatibleDC(screen_dc);

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as DWORD;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32); // Top-down DIB
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut c_void = null_mut();
        let hbmp = CreateDIBSection(screen_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if !hbmp.is_null() && !bits.is_null() {
            copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
            let old_bmp = SelectObject(mem_dc, hbmp as _);

            let mut sz = SIZE {
                cx: w as i32,
                cy: h as i32,
            };
            let mut pt_src = POINT { x: 0, y: 0 };
            let mut blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA,
            };

            UpdateLayeredWindow(
                state.hwnd,
                screen_dc,
                null_mut(),
                &mut sz,
                mem_dc,
                &mut pt_src,
                0,
                &mut blend,
                ULW_ALPHA,
            );

            SelectObject(mem_dc, old_bmp);
            DeleteObject(hbmp as _);
        }

        DeleteDC(mem_dc);
        ReleaseDC(null_mut(), screen_dc);
    }
}

unsafe fn is_shell_window_covering_us(widget: HWND, shell_tray: HWND) -> bool {
    let mut cur = unsafe { GetWindow(widget, GW_HWNDPREV) };
    for _ in 0..64 {
        if cur.is_null() {
            break;
        }
        if cur == shell_tray {
            return true;
        }
        cur = unsafe { GetWindow(cur, GW_HWNDPREV) };
    }
    false
}

unsafe fn is_fullscreen_on_tray_monitor(shell_tray: HWND, tray_rc: &RECT) -> bool {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_null() || fg == shell_tray {
        return false;
    }

    let mut cls_buf = [0u16; 128];
    let len = unsafe { GetClassNameW(fg, cls_buf.as_mut_ptr(), cls_buf.len() as i32) };
    let cls = String::from_utf16_lossy(&cls_buf[..len.max(0) as usize]);
    if matches!(
        cls.as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
            | "Xaml_WindowedPopupClass"
    ) {
        return false;
    }

    let tray_mon = unsafe { MonitorFromWindow(shell_tray, MONITOR_DEFAULTTONEAREST) };
    let fg_mon = unsafe { MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) };
    if tray_mon != fg_mon {
        return false;
    }

    let mut mi: MONITORINFO = unsafe { zeroed() };
    mi.cbSize = size_of::<MONITORINFO>() as DWORD;
    if unsafe { GetMonitorInfoW(tray_mon, &mut mi) } == 0 {
        return false;
    }

    let mut fg_rc: RECT = unsafe { zeroed() };
    if unsafe { GetWindowRect(fg, &mut fg_rc) } == 0 {
        return false;
    }

    // Maximized windows have an invisible resize border (~8px) that extends outside rcWork,
    // so their bottom stays near tray_rc.top. A true fullscreen window covers the entire monitor
    // AND extends all the way to the bottom of the taskbar (and sits above Shell_TrayWnd in Z-order).
    let covers_monitor = fg_rc.left <= mi.rcMonitor.left
        && fg_rc.top <= mi.rcMonitor.top
        && fg_rc.right >= mi.rcMonitor.right
        && fg_rc.bottom >= tray_rc.bottom;

    if !covers_monitor {
        return false;
    }

    // Verify fg is actually above Shell_TrayWnd in Z-order (true fullscreen), not behind it
    let mut cur = unsafe { GetWindow(shell_tray, GW_HWNDPREV) };
    for _ in 0..64 {
        if cur.is_null() {
            break;
        }
        if cur == fg {
            return true;
        }
        cur = unsafe { GetWindow(cur, GW_HWNDPREV) };
    }
    false
}

unsafe fn sync_position_and_visibility(state: &mut WidgetState, force: bool) {
    let tray_cls = to_wide("Shell_TrayWnd");
    let shell_tray = unsafe { FindWindowW(tray_cls.as_ptr(), null_mut()) };
    if shell_tray.is_null() || unsafe { IsWindowVisible(shell_tray) } == 0 {
        if state.visible {
            unsafe { ShowWindow(state.hwnd, SW_HIDE) };
            state.visible = false;
        }
        return;
    }

    let notify_cls = to_wide("TrayNotifyWnd");
    let tray_notify = unsafe { FindWindowExW(shell_tray, null_mut(), notify_cls.as_ptr(), null_mut()) };
    if tray_notify.is_null() {
        if state.visible {
            unsafe { ShowWindow(state.hwnd, SW_HIDE) };
            state.visible = false;
        }
        return;
    }

    let mut tray_rc: RECT = unsafe { zeroed() };
    let mut notify_rc: RECT = unsafe { zeroed() };
    unsafe {
        GetWindowRect(shell_tray, &mut tray_rc);
        GetWindowRect(tray_notify, &mut notify_rc);
    }

    // Check if taskbar is auto-hidden off-screen or a fullscreen app is active
    let tray_mon = unsafe { MonitorFromWindow(shell_tray, MONITOR_DEFAULTTONEAREST) };
    let mut mi: MONITORINFO = unsafe { zeroed() };
    mi.cbSize = size_of::<MONITORINFO>() as DWORD;
    let monitor_ok = unsafe { GetMonitorInfoW(tray_mon, &mut mi) } != 0;
    let auto_hidden = monitor_ok && tray_rc.top >= mi.rcMonitor.bottom - 4;

    if auto_hidden
        || unsafe { is_fullscreen_on_tray_monitor(shell_tray, &tray_rc) }
        || notify_rc.left <= 0
    {
        if state.visible {
            unsafe { ShowWindow(state.hwnd, SW_HIDE) };
            state.visible = false;
        }
        return;
    }

    let dpi = unsafe { GetDpiForWindow(shell_tray) };
    let new_scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    let scale_changed = (new_scale - state.scale).abs() > 0.01;
    let light_now = is_light_theme();
    let theme_changed = light_now != state.last_light_theme;

    if scale_changed || theme_changed {
        state.scale = new_scale;
        state.last_light_theme = light_now;
        unsafe {
            render_widget(state);
            update_tooltip_rect(state);
        }
    }

    let gap = (1.0 * state.scale).round() as i32;
    let new_x = (notify_rc.left - gap - state.width_px).max(0);
    let tray_h = tray_rc.bottom - tray_rc.top;
    let new_y = tray_rc.top + (tray_h - state.height_px) / 2;

    let pos_changed = force
        || new_x != state.pos_x
        || new_y != state.pos_y
        || scale_changed
        || !state.visible;

    let covered = unsafe { is_shell_window_covering_us(state.hwnd, shell_tray) };

    if pos_changed || covered {
        state.pos_x = new_x;
        state.pos_y = new_y;
        unsafe {
            SetWindowPos(
                state.hwnd,
                HWND_TOPMOST,
                new_x,
                new_y,
                state.width_px,
                state.height_px,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            if !state.visible {
                ShowWindow(state.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        state.visible = true;
    }
}

unsafe fn show_context_menu(state: &WidgetState) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }

        let refresh_txt = to_wide("Làm mới (Refresh)");
        let autostart_txt = to_wide("Khởi động cùng Windows");
        let quit_txt = to_wide("Thoát (Exit)");

        AppendMenuW(menu, MF_STRING, CMD_REFRESH, refresh_txt.as_ptr());
        let auto_flags = if state.autostart_enabled.load(Ordering::SeqCst) {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        AppendMenuW(menu, auto_flags, CMD_AUTOSTART, autostart_txt.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null_mut());
        AppendMenuW(menu, MF_STRING, CMD_QUIT, quit_txt.as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(state.hwnd);

        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            pt.x,
            pt.y,
            0,
            state.hwnd,
            null_mut(),
        );
        DestroyMenu(menu);

        // Keep widget above taskbar after menu closes
        SetWindowPos(
            state.hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        );

        match cmd as usize {
            CMD_REFRESH => {
                PostThreadMessageW(state.main_thread_id, WM_WIDGET_REFRESH, 0, 0);
            }
            CMD_AUTOSTART => {
                PostThreadMessageW(state.main_thread_id, WM_WIDGET_TOGGLE_AUTOSTART, 0, 0);
            }
            CMD_QUIT => {
                PostThreadMessageW(state.main_thread_id, WM_WIDGET_QUIT, 0, 0);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn widget_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WidgetState;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = &mut *ptr;

        match msg {
            WM_TIMER => {
                if wparam == TIMER_SYNC_ID {
                    sync_position_and_visibility(state, false);
                }
                0
            }
            WM_MOUSEMOVE => {
                if !state.hovered {
                    state.hovered = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as DWORD,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    render_widget(state);
                }
                0
            }
            WM_MOUSELEAVE => {
                if state.hovered {
                    state.hovered = false;
                    render_widget(state);
                }
                0
            }
            WM_LBUTTONUP => {
                PostThreadMessageW(state.main_thread_id, WM_WIDGET_REFRESH, 0, 0);
                0
            }
            WM_RBUTTONUP => {
                show_context_menu(state);
                0
            }
            WM_SETTINGCHANGE | WM_DPICHANGED => {
                sync_position_and_visibility(state, true);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
