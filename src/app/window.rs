use crate::tray::AppContext;
use logipeek::app::{
    settings::{Settings, Theme},
    slider,
    state::{DeviceStatus, OperationStatus},
};
use logipeek::hid::features::battery::Level;
use std::{
    cell::Cell,
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{null, null_mut},
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
        Gdi::{
            BeginPaint, COLOR_WINDOW, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
            DeleteObject, Ellipse, EndPaint, FillRect, GetSysColor, GetTextExtentPoint32W, HDC,
            HFONT, InvalidateRect, LineTo, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID,
            RoundRect, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
        },
    },
    UI::{
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow},
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_F5, VK_RETURN, VK_TAB},
        WindowsAndMessaging::{
            CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
            GWLP_USERDATA, GetClientRect, GetSystemMetrics, GetWindowLongPtrW, HICON, ICON_BIG,
            ICON_SMALL, IDC_ARROW, IDC_HAND, LoadCursorW, RegisterClassExW, SM_CXSCREEN,
            SM_CYSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW, SendMessageW, SetCursor,
            SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, UnregisterClassW,
            WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DPICHANGED,
            WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
            WM_NCDESTROY, WM_PAINT, WM_SETICON, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SYSCOLORCHANGE,
            WM_THEMECHANGED, WNDCLASSEXW, WS_CAPTION, WS_EX_APPWINDOW, WS_MINIMIZEBOX,
            WS_OVERLAPPED, WS_SYSMENU,
        },
    },
};

const CLASS_NAME: &str = "LogiPeek.Settings.Window";
const CLIENT_WIDTH: i32 = 440;
const CLIENT_HEIGHT: i32 = 660;
const WM_REDRAW: u32 = WM_APP + 10;
const REFRESH_RECT: (i32, i32, i32, i32) = (380, 22, 36, 36);
const SLIDER_RECT: (i32, i32, i32, i32) = (38, 330, 364, 40);
const SAVE_RECT: (i32, i32, i32, i32) = (300, 603, 104, 30);

pub(crate) struct MainWindow {
    hwnd: HWND,
    instance: HINSTANCE,
    class_name: Vec<u16>,
    state: Box<WindowState>,
}

struct WindowState {
    app: *const AppContext,
    hwnd: Cell<HWND>,
    dpi: Cell<u32>,
    drafts: Mutex<[String; 4]>,
    editing: Cell<Option<usize>>,
    dragging: Cell<bool>,
    preview: Cell<Option<u16>>,
    pending_theme: Cell<Theme>,
    hover: Cell<Option<HitTarget>>,
    pressed: Cell<Option<HitTarget>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    Refresh,
    Slider,
    Preset(usize),
    Edit(usize),
    Theme(usize),
    Save,
}

#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    card: u32,
    card_outline: u32,
    highlight: u32,
    shadow: u32,
    text: u32,
    secondary: u32,
    accent: u32,
    track: u32,
    edit: u32,
    pill: u32,
    pill_hover: u32,
    pill_pressed: u32,
    selected_outline: u32,
    status_ok: u32,
    status_warn: u32,
}

struct Button<'a> {
    rect: (i32, i32, i32, i32),
    label: &'a str,
    enabled: bool,
    selected: bool,
    hovered: bool,
    pressed: bool,
}

impl MainWindow {
    pub(crate) fn create(
        instance: HINSTANCE,
        app: &AppContext,
        icon: HICON,
        small_icon: HICON,
    ) -> Result<Self, String> {
        let class_name = wide(CLASS_NAME);
        let window_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: unsafe {
                // SAFETY: IDC_ARROW is a system cursor with process-independent lifetime.
                LoadCursorW(null_mut(), IDC_ARROW)
            },
            hIcon: icon,
            hIconSm: small_icon,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if unsafe {
            // SAFETY: The class descriptor and class name stay valid through this call.
            RegisterClassExW(&window_class)
        } == 0
        {
            return Err("Could not register the LogiPeek settings window class".into());
        }

        let dpi = unsafe {
            // SAFETY: Process DPI awareness is configured before this call.
            GetDpiForSystem()
        }
        .max(96);
        let snapshot = app.snapshot();
        let mut state = Box::new(WindowState {
            app: app as *const AppContext,
            hwnd: Cell::new(null_mut()),
            dpi: Cell::new(dpi),
            drafts: Mutex::new(snapshot.presets.map(|value| value.to_string())),
            editing: Cell::new(None),
            dragging: Cell::new(false),
            preview: Cell::new(None),
            pending_theme: Cell::new(snapshot.theme),
            hover: Cell::new(None),
            pressed: Cell::new(None),
        });
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: scale(CLIENT_WIDTH, dpi),
            bottom: scale(CLIENT_HEIGHT, dpi),
        };
        unsafe {
            // SAFETY: bounds is writable and the style matches CreateWindowExW below.
            AdjustWindowRectExForDpi(&mut bounds, style, 0, WS_EX_APPWINDOW, dpi);
        }
        let title = wide("LogiPeek");
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let x = unsafe {
            // SAFETY: GetSystemMetrics has no pointer inputs and returns the primary screen size.
            (GetSystemMetrics(SM_CXSCREEN) - width).max(0) / 2
        };
        let y = unsafe {
            // SAFETY: GetSystemMetrics has no pointer inputs and returns the primary screen size.
            (GetSystemMetrics(SM_CYSCREEN) - height).max(0) / 2
        };
        let hwnd = unsafe {
            // SAFETY: lpParam points to the stable WindowState box retained by MainWindow.
            CreateWindowExW(
                WS_EX_APPWINDOW,
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                x,
                y,
                width,
                height,
                null_mut(),
                null_mut(),
                instance,
                (&mut *state as *mut WindowState).cast::<c_void>(),
            )
        };
        if hwnd.is_null() {
            unsafe {
                // SAFETY: No window survived creation, so the class can be released.
                UnregisterClassW(class_name.as_ptr(), instance);
            }
            return Err("Could not create the LogiPeek settings window".into());
        }
        let window_dpi = unsafe {
            // SAFETY: hwnd is a newly created live top-level window.
            GetDpiForWindow(hwnd)
        }
        .max(96);
        if window_dpi != dpi {
            state.dpi.set(window_dpi);
            let mut corrected = RECT {
                left: 0,
                top: 0,
                right: scale(CLIENT_WIDTH, window_dpi),
                bottom: scale(CLIENT_HEIGHT, window_dpi),
            };
            unsafe {
                // SAFETY: corrected is writable and uses the same styles as the live window.
                AdjustWindowRectExForDpi(&mut corrected, style, 0, WS_EX_APPWINDOW, window_dpi);
                let corrected_width = corrected.right - corrected.left;
                let corrected_height = corrected.bottom - corrected.top;
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    (GetSystemMetrics(SM_CXSCREEN) - corrected_width).max(0) / 2,
                    (GetSystemMetrics(SM_CYSCREEN) - corrected_height).max(0) / 2,
                    corrected_width,
                    corrected_height,
                    0,
                );
            }
        }
        unsafe {
            // SAFETY: Both icons outlive this window and remain owned by the application context.
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, icon as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small_icon as isize);
        }
        app.set_main_hwnd(hwnd);
        Ok(Self {
            hwnd,
            instance,
            class_name,
            state,
        })
    }

    pub(crate) fn show(&self) {
        show(self.hwnd);
    }

    pub(crate) fn destroy(self) {
        unsafe {
            // SAFETY: MainWindow owns the live top-level window and registered class.
            DestroyWindow(self.hwnd);
            UnregisterClassW(self.class_name.as_ptr(), self.instance);
        }
        drop(self.state);
    }
}

pub(crate) fn show(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        // SAFETY: hwnd is the application-owned settings window; these calls do not transfer ownership.
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        InvalidateRect(hwnd, null(), 0);
    }
}

pub(crate) fn state_changed(hwnd: HWND) {
    if !hwnd.is_null() {
        unsafe {
            // SAFETY: Only an integer notification is queued; a stale HWND fails harmlessly.
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, WM_REDRAW, 0, 0);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        // SAFETY: The inner handler validates GWLP_USERDATA before dereferencing it.
        window_proc_inner(hwnd, message, wparam, lparam)
    }))
    .unwrap_or_else(|_| unsafe {
        // SAFETY: Panics never cross the FFI boundary; Windows receives the default result.
        DefWindowProcW(hwnd, message, wparam, lparam)
    })
}

unsafe fn window_proc_inner(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        let create = lparam as *const CREATESTRUCTW;
        if !create.is_null() {
            let pointer = unsafe {
                // SAFETY: WM_NCCREATE lParam points to a valid CREATESTRUCTW for this call.
                (*create).lpCreateParams as *mut WindowState
            };
            unsafe {
                // SAFETY: The pointer comes from MainWindow::create and remains boxed through destruction.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
            }
            if !pointer.is_null() {
                unsafe {
                    // SAFETY: Only Cell interior state is updated through this shared object.
                    (&*pointer).hwnd.set(hwnd);
                }
            }
        }
    }
    let pointer = unsafe {
        // SAFETY: Reading GWLP_USERDATA does not dereference the stored pointer.
        GetWindowLongPtrW(hwnd, GWLP_USERDATA)
    } as *const WindowState;
    if pointer.is_null() {
        return unsafe {
            // SAFETY: Messages before WM_NCCREATE completes use the Windows default procedure.
            DefWindowProcW(hwnd, message, wparam, lparam)
        };
    }
    let state = unsafe {
        // SAFETY: MainWindow owns a stable Box until after DestroyWindow returns. WndProc creates
        // shared references only; mutable UI scalars use Cell, including across reentrant calls.
        &*pointer
    };
    match message {
        WM_CREATE => {
            apply_theme(state);
            return 0;
        }
        WM_PAINT => {
            paint(state);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_DPICHANGED => {
            let new_dpi = ((wparam >> 16) as u32).max(96);
            state.dpi.set(new_dpi);
            let suggested = lparam as *const RECT;
            if !suggested.is_null() {
                let rect = unsafe {
                    // SAFETY: WM_DPICHANGED supplies a valid suggested RECT for this call.
                    *suggested
                };
                let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
                let mut bounds = RECT {
                    left: 0,
                    top: 0,
                    right: scale(CLIENT_WIDTH, new_dpi),
                    bottom: scale(CLIENT_HEIGHT, new_dpi),
                };
                unsafe {
                    // SAFETY: bounds is writable and describes this window's fixed logical client
                    // size at the new DPI. The suggested position keeps the window on its monitor.
                    AdjustWindowRectExForDpi(&mut bounds, style, 0, WS_EX_APPWINDOW, new_dpi);
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        rect.left,
                        rect.top,
                        bounds.right - bounds.left,
                        bounds.bottom - bounds.top,
                        0,
                    );
                }
            }
            invalidate(hwnd);
            return 0;
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point_from_lparam(lparam);
            if slider_hit(state, x, y) {
                begin_slider(state, x);
            } else {
                state.pressed.set(hit_target(state, x, y));
                invalidate(hwnd);
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            if state.dragging.get() {
                preview_slider(state, point_from_lparam(lparam).0);
            } else {
                update_hover(state, point_from_lparam(lparam));
            }
            return 0;
        }
        WM_LBUTTONUP => {
            let (x, y) = point_from_lparam(lparam);
            if state.dragging.replace(false) {
                preview_slider(state, x);
                let committed = state.preview.get();
                unsafe {
                    // SAFETY: This thread acquired capture at gesture start. ReleaseCapture can
                    // synchronously clear the shared preview through WM_CAPTURECHANGED, so the
                    // value to commit was copied above.
                    ReleaseCapture();
                }
                if let Some(value) = committed {
                    commit_slider(state, value);
                }
            } else {
                let released = hit_target(state, x, y);
                let pressed = state.pressed.replace(None);
                if released.is_some() && released == pressed {
                    handle_target(state, released.unwrap_or(HitTarget::Refresh));
                }
                invalidate(hwnd);
            }
            return 0;
        }
        WM_CAPTURECHANGED => {
            state.dragging.set(false);
            state.preview.set(None);
            invalidate(hwnd);
            return 0;
        }
        WM_KEYDOWN => {
            if wparam as u16 == VK_F5 {
                app(state).refresh();
            } else if wparam as u16 == VK_RETURN {
                save_from_controls(state);
            } else if wparam as u16 == VK_TAB {
                let next = state.editing.get().map_or(0, |index| (index + 1) % 4);
                state.editing.set(Some(next));
                invalidate(hwnd);
            }
            return 0;
        }
        WM_CHAR => {
            edit_character(state, wparam as u32);
            return 0;
        }
        WM_COMMAND => return 0,
        WM_REDRAW => {
            let snapshot = app(state).snapshot();
            if !matches!(snapshot.operation, OperationStatus::Applying(_)) && !state.dragging.get()
            {
                state.preview.set(None);
            }
            invalidate(hwnd);
            return 0;
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => {
            if state.pending_theme.get() == Theme::System {
                apply_theme(state);
                invalidate(hwnd);
            }
            return 0;
        }
        WM_SHOWWINDOW => {
            if wparam != 0 {
                apply_theme(state);
                invalidate(hwnd);
            }
            return 0;
        }
        WM_CLOSE => {
            if state.dragging.replace(false) {
                unsafe {
                    // SAFETY: Closing cancels the in-progress gesture. Capture is released without
                    // committing so a hidden window cannot retain mouse input.
                    ReleaseCapture();
                }
            }
            state.preview.set(None);
            unsafe {
                // SAFETY: Hiding the owned settings window implements close-to-tray.
                ShowWindow(hwnd, SW_HIDE);
            }
            return 0;
        }
        WM_NCDESTROY => {
            app(state).set_main_hwnd(null_mut());
            unsafe {
                // SAFETY: Clear the non-owning pointer before the WindowState box can be dropped.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
        }
        _ => {}
    }
    unsafe {
        // SAFETY: Unhandled messages are delegated to the Windows default procedure.
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

fn handle_target(state: &WindowState, target: HitTarget) {
    let snapshot = app(state).snapshot();
    match target {
        HitTarget::Refresh => app(state).refresh(),
        HitTarget::Edit(index) => {
            state.editing.set(Some(index));
            unsafe {
                // SAFETY: Focusing the owned top-level window enables direct numeric editing.
                SetFocus(state.hwnd.get());
            }
            invalidate(state.hwnd.get());
        }
        HitTarget::Preset(index) => {
            let preset = snapshot.presets()[index];
            if preset.enabled {
                app(state).submit_dpi(preset.dpi);
            }
        }
        HitTarget::Theme(index) => {
            let theme = [Theme::System, Theme::Light, Theme::Dark][index];
            state.pending_theme.set(theme);
            apply_theme(state);
            invalidate(state.hwnd.get());
        }
        HitTarget::Save => save_from_controls(state),
        HitTarget::Slider => {}
    }
}

fn hit_target(state: &WindowState, x: i32, y: i32) -> Option<HitTarget> {
    let dpi = state.dpi.get();
    let logical = (unscale(x, dpi), unscale(y, dpi));
    if contains(REFRESH_RECT, logical) {
        return Some(HitTarget::Refresh);
    }
    if contains(SLIDER_RECT, logical) {
        return Some(HitTarget::Slider);
    }
    for index in 0..4 {
        if contains(preset_rect(index), logical) {
            return Some(HitTarget::Preset(index));
        }
        if contains(edit_rect(index), logical) {
            return Some(HitTarget::Edit(index));
        }
    }
    for index in 0..3 {
        if contains(theme_rect(index), logical) {
            return Some(HitTarget::Theme(index));
        }
    }
    contains(SAVE_RECT, logical).then_some(HitTarget::Save)
}

fn update_hover(state: &WindowState, point: (i32, i32)) {
    let target = hit_target(state, point.0, point.1);
    if state.hover.replace(target) != target {
        invalidate(state.hwnd.get());
    }
    let cursor = if target.is_some() {
        IDC_HAND
    } else {
        IDC_ARROW
    };
    unsafe {
        // SAFETY: Both identifiers select shared system cursors; the window does not own them.
        SetCursor(LoadCursorW(null_mut(), cursor));
    }
}

fn begin_slider(state: &WindowState, x: i32) {
    let snapshot = app(state).snapshot();
    if snapshot.status != DeviceStatus::Single || snapshot.supported_dpi.is_none() {
        return;
    }
    state.dragging.set(true);
    unsafe {
        // SAFETY: Capture is scoped to this UI-thread gesture and released on completion/cancel.
        SetCapture(state.hwnd.get());
    }
    preview_slider(state, x);
}

fn preview_slider(state: &WindowState, x: i32) {
    let snapshot = app(state).snapshot();
    let Some(values) = snapshot.supported_dpi.as_ref() else {
        state.preview.set(None);
        return;
    };
    let dpi = state.dpi.get();
    let left = scale(48, dpi);
    let width = scale(344, dpi).max(1);
    state.preview.set(slider::value_at(values, x - left, width));
    invalidate(state.hwnd.get());
}

fn commit_slider(state: &WindowState, value: u16) {
    let snapshot = app(state).snapshot();
    if snapshot.current_dpi != Some(value) {
        app(state).submit_dpi(value);
    } else {
        state.preview.set(None);
        invalidate(state.hwnd.get());
    }
}

fn save_from_controls(state: &WindowState) {
    let mut presets = [0u16; 4];
    let Ok(drafts) = state.drafts.lock() else {
        app(state).set_notice("Preset editor is unavailable");
        return;
    };
    for (index, draft) in drafts.iter().enumerate() {
        let Some(value) = draft.trim().parse::<u16>().ok() else {
            app(state).set_notice("Presets must be 1-65535");
            return;
        };
        if value == 0 {
            app(state).set_notice("Presets must be 1-65535");
            return;
        }
        presets[index] = value;
    }
    drop(drafts);
    app(state).save_settings(Settings {
        presets,
        theme: state.pending_theme.get(),
    });
}

fn edit_character(state: &WindowState, character: u32) {
    let Some(index) = state.editing.get() else {
        return;
    };
    if character == u32::from(b'\r') {
        save_from_controls(state);
        return;
    }
    let Ok(mut drafts) = state.drafts.lock() else {
        return;
    };
    if character == u32::from(b'\x08') {
        drafts[index].pop();
    } else if let Some(character) = char::from_u32(character)
        && character.is_ascii_digit()
        && drafts[index].len() < 5
    {
        drafts[index].push(character);
    }
    drop(drafts);
    invalidate(state.hwnd.get());
}

fn paint(state: &WindowState) {
    let hwnd = state.hwnd.get();
    let mut paint = PAINTSTRUCT::default();
    let dc = unsafe {
        // SAFETY: BeginPaint is paired with EndPaint below for this WM_PAINT.
        BeginPaint(hwnd, &mut paint)
    };
    if dc.is_null() {
        return;
    }
    let dpi = state.dpi.get();
    let snapshot = app(state).snapshot();
    let palette = palette(state.pending_theme.get());
    let mut client = RECT::default();
    unsafe {
        // SAFETY: client is writable and hwnd is the live settings window.
        GetClientRect(hwnd, &mut client);
    }
    fill(dc, client, palette.background);

    let title = font(dpi, 24, 600);
    let heading = font(dpi, 13, 600);
    let body = font(dpi, 13, 400);
    let secondary = font(dpi, 11, 400);
    let dpi_font = font(dpi, 39, 600);
    let battery_font = font(dpi, 22, 600);
    draw_text(
        dc,
        title,
        palette.text,
        scale(24, dpi),
        scale(18, dpi),
        "LogiPeek",
    );
    draw_button(
        dc,
        body,
        palette,
        dpi,
        Button {
            rect: REFRESH_RECT,
            label: "↻",
            enabled: true,
            selected: false,
            hovered: state.hover.get() == Some(HitTarget::Refresh),
            pressed: state.pressed.get() == Some(HitTarget::Refresh),
        },
    );

    draw_text(
        dc,
        body,
        palette.text,
        scale(24, dpi),
        scale(56, dpi),
        "Logitech Mouse",
    );
    let status_color = match snapshot.status {
        DeviceStatus::Single => palette.status_ok,
        DeviceStatus::Unavailable | DeviceStatus::MultipleDevices => palette.status_warn,
    };
    circle(
        dc,
        status_color,
        scale(29, dpi),
        scale(85, dpi),
        scale(3, dpi).max(2),
    );
    draw_text(
        dc,
        secondary,
        palette.secondary,
        scale(39, dpi),
        scale(78, dpi),
        snapshot.connection_text(),
    );

    draw_card(dc, palette, dpi, (24, 108, 392, 108));
    draw_text(
        dc,
        heading,
        palette.text,
        scale(42, dpi),
        scale(126, dpi),
        "Battery",
    );
    let battery = snapshot.battery_value_text();
    draw_text_center(
        dc,
        battery_font,
        palette.text,
        dpi_rect((326, 117, 72, 36), dpi),
        &battery,
    );
    if let Some(percent) = snapshot.battery_percent {
        draw_progress(dc, palette, dpi, percent);
    }
    let battery_detail = battery_detail(&snapshot);
    draw_text(
        dc,
        secondary,
        palette.secondary,
        scale(42, dpi),
        scale(186, dpi),
        &battery_detail,
    );

    draw_card(dc, palette, dpi, (24, 232, 392, 220));
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(42, dpi),
        scale(250, dpi),
        "POINTER SPEED",
    );
    let displayed =
        if state.dragging.get() || matches!(snapshot.operation, OperationStatus::Applying(_)) {
            state.preview.get().or(snapshot.current_dpi)
        } else {
            snapshot.current_dpi
        };
    draw_text_center(
        dc,
        dpi_font,
        palette.text,
        dpi_rect((92, 262, 256, 54), dpi),
        &displayed.map_or_else(|| "—".into(), |value| value.to_string()),
    );
    draw_text_center(
        dc,
        secondary,
        palette.secondary,
        dpi_rect((160, 310, 120, 20), dpi),
        "DPI",
    );
    draw_slider(dc, &snapshot, palette, dpi, displayed);
    for (index, preset) in snapshot.presets().into_iter().enumerate() {
        let label = preset.dpi.to_string();
        let target = HitTarget::Preset(index);
        draw_button(
            dc,
            body,
            palette,
            dpi,
            Button {
                rect: preset_rect(index),
                label: &label,
                enabled: preset.enabled,
                selected: preset.checked,
                hovered: state.hover.get() == Some(target),
                pressed: state.pressed.get() == Some(target),
            },
        );
    }
    let operation = snapshot.operation_text();
    if !operation.is_empty() {
        draw_text(
            dc,
            secondary,
            palette.secondary,
            scale(42, dpi),
            scale(378, dpi),
            &operation,
        );
    }

    draw_card(dc, palette, dpi, (24, 468, 392, 172));
    draw_text(
        dc,
        heading,
        palette.text,
        scale(42, dpi),
        scale(482, dpi),
        "Presets",
    );
    let drafts = state
        .drafts
        .lock()
        .map(|drafts| drafts.clone())
        .unwrap_or_else(|_| snapshot.presets.map(|value| value.to_string()));
    for (index, label) in drafts.iter().enumerate() {
        let target = HitTarget::Edit(index);
        draw_button(
            dc,
            body,
            Palette {
                pill: palette.edit,
                ..palette
            },
            dpi,
            Button {
                rect: edit_rect(index),
                label,
                enabled: true,
                selected: state.editing.get() == Some(index),
                hovered: state.hover.get() == Some(target),
                pressed: state.pressed.get() == Some(target),
            },
        );
    }
    draw_text(
        dc,
        heading,
        palette.text,
        scale(42, dpi),
        scale(548, dpi),
        "Appearance",
    );
    for (index, (theme, label)) in [
        (Theme::System, "System"),
        (Theme::Light, "Light"),
        (Theme::Dark, "Dark"),
    ]
    .into_iter()
    .enumerate()
    {
        let target = HitTarget::Theme(index);
        draw_button(
            dc,
            body,
            palette,
            dpi,
            Button {
                rect: theme_rect(index),
                label,
                enabled: true,
                selected: state.pending_theme.get() == theme,
                hovered: state.hover.get() == Some(target),
                pressed: state.pressed.get() == Some(target),
            },
        );
    }
    draw_button(
        dc,
        body,
        palette,
        dpi,
        Button {
            rect: SAVE_RECT,
            label: "Save",
            enabled: true,
            selected: true,
            hovered: state.hover.get() == Some(HitTarget::Save),
            pressed: state.pressed.get() == Some(HitTarget::Save),
        },
    );
    if let Some(notice) = snapshot.settings_notice.as_deref() {
        draw_text(
            dc,
            secondary,
            palette.secondary,
            scale(42, dpi),
            scale(611, dpi),
            notice,
        );
    }
    draw_text(
        dc,
        secondary,
        palette.secondary,
        scale(24, dpi),
        scale(635, dpi),
        "Closing this window keeps LogiPeek running in the tray.",
    );

    for object in [title, heading, body, secondary, dpi_font, battery_font] {
        if !object.is_null() {
            unsafe {
                // SAFETY: Each font was created for this paint and is not selected after draw_text.
                DeleteObject(object);
            }
        }
    }
    unsafe {
        // SAFETY: Completes the BeginPaint call above.
        EndPaint(hwnd, &paint);
    }
}

fn draw_slider(
    dc: HDC,
    snapshot: &logipeek::app::state::AppState,
    palette: Palette,
    dpi: u32,
    displayed: Option<u16>,
) {
    let left = scale(48, dpi);
    let right = scale(392, dpi);
    let y = scale(350, dpi);
    line(dc, palette.track, scale(3, dpi).max(1), left, y, right, y);
    if let Some(values) = snapshot.supported_dpi.as_ref() {
        if let Some(value) = displayed
            && let Some(position) = slider::position_of(values, value, right - left)
        {
            line(
                dc,
                palette.accent,
                scale(3, dpi).max(1),
                left,
                y,
                left + position,
                y,
            );
            circle(dc, palette.card, left + position, y, scale(10, dpi));
            circle(dc, palette.accent, left + position, y, scale(7, dpi));
        }
        if let Some((minimum, maximum)) = slider::endpoints(values) {
            let small = font(dpi, 11, 400);
            draw_text(
                dc,
                small,
                palette.secondary,
                left,
                scale(365, dpi),
                &minimum.to_string(),
            );
            draw_text(
                dc,
                small,
                palette.secondary,
                scale(350, dpi),
                scale(365, dpi),
                &maximum.to_string(),
            );
            if !small.is_null() {
                unsafe {
                    // SAFETY: small was created locally and is no longer selected.
                    DeleteObject(small);
                }
            }
        }
    } else {
        let small = font(dpi, 12, 400);
        draw_text(
            dc,
            small,
            palette.secondary,
            left,
            scale(365, dpi),
            "DPI controls unavailable",
        );
        if !small.is_null() {
            unsafe {
                // SAFETY: small was created locally and is no longer selected.
                DeleteObject(small);
            }
        }
    }
}

fn draw_progress(dc: HDC, palette: Palette, dpi: u32, percent: u8) {
    let track = RECT {
        left: scale(42, dpi),
        top: scale(166, dpi),
        right: scale(398, dpi),
        bottom: scale(174, dpi),
    };
    rounded_fill(dc, track, palette.track, scale(8, dpi));
    let filled = RECT {
        right: track.left + (track.right - track.left) * i32::from(percent.min(100)) / 100,
        ..track
    };
    rounded_fill(dc, filled, palette.accent, scale(8, dpi));
}

fn draw_card(dc: HDC, palette: Palette, dpi: u32, rect: (i32, i32, i32, i32)) {
    let shadow = dpi_rect((rect.0 + 2, rect.1 + 3, rect.2, rect.3), dpi);
    rounded_fill(dc, shadow, palette.shadow, scale(20, dpi));
    let outer = dpi_rect(rect, dpi);
    rounded_fill(dc, outer, palette.card_outline, scale(20, dpi));
    let inner = RECT {
        left: outer.left + scale(1, dpi).max(1),
        top: outer.top + scale(1, dpi).max(1),
        right: outer.right - scale(1, dpi).max(1),
        bottom: outer.bottom - scale(1, dpi).max(1),
    };
    rounded_fill(dc, inner, palette.card, scale(19, dpi));
    line(
        dc,
        palette.highlight,
        scale(1, dpi).max(1),
        scale(rect.0 + 20, dpi),
        scale(rect.1 + 2, dpi),
        scale(rect.0 + rect.2 - 20, dpi),
        scale(rect.1 + 2, dpi),
    );
}

fn draw_button(dc: HDC, font: HFONT, palette: Palette, dpi: u32, button: Button<'_>) {
    let background = if button.pressed {
        palette.pill_pressed
    } else if button.selected {
        palette.accent
    } else if button.hovered {
        palette.pill_hover
    } else {
        palette.pill
    };
    let rect = dpi_rect(button.rect, dpi);
    rounded_fill(
        dc,
        rect,
        if button.selected {
            palette.selected_outline
        } else {
            palette.card_outline
        },
        rect.bottom - rect.top,
    );
    let inset = scale(1, dpi).max(1);
    rounded_fill(
        dc,
        RECT {
            left: rect.left + inset,
            top: rect.top + inset,
            right: rect.right - inset,
            bottom: rect.bottom - inset,
        },
        background,
        rect.bottom - rect.top - inset * 2,
    );
    let color = if button.selected {
        rgb(255, 255, 255)
    } else if button.enabled {
        palette.text
    } else {
        palette.secondary
    };
    draw_text_center(dc, font, color, rect, button.label);
}

fn rounded_fill(dc: HDC, rect: RECT, color: u32, diameter: i32) {
    let brush = unsafe { CreateSolidBrush(color) };
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    if brush.is_null() || pen.is_null() {
        unsafe {
            // SAFETY: Delete only local GDI objects that were successfully created.
            if !brush.is_null() {
                DeleteObject(brush);
            }
            if !pen.is_null() {
                DeleteObject(pen);
            }
        }
        return;
    }
    let old_brush = unsafe { SelectObject(dc, brush) };
    let old_pen = unsafe { SelectObject(dc, pen) };
    unsafe {
        RoundRect(
            dc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            diameter.max(1),
            diameter.max(1),
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

fn draw_text(dc: HDC, font: HFONT, color: u32, x: i32, y: i32, text: &str) {
    if font.is_null() {
        return;
    }
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let old_font = unsafe { SelectObject(dc, font) };
    unsafe {
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        TextOutW(dc, x, y, encoded.as_ptr(), encoded.len() as i32);
        SelectObject(dc, old_font);
    }
}

fn draw_text_center(dc: HDC, font: HFONT, color: u32, rect: RECT, text: &str) {
    if font.is_null() {
        return;
    }
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let old_font = unsafe { SelectObject(dc, font) };
    let mut size = SIZE::default();
    unsafe {
        GetTextExtentPoint32W(dc, encoded.as_ptr(), encoded.len() as i32, &mut size);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, color);
        TextOutW(
            dc,
            rect.left + ((rect.right - rect.left - size.cx) / 2).max(0),
            rect.top + ((rect.bottom - rect.top - size.cy) / 2).max(0),
            encoded.as_ptr(),
            encoded.len() as i32,
        );
        SelectObject(dc, old_font);
    }
}

fn fill(dc: HDC, rect: RECT, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    if !brush.is_null() {
        unsafe {
            FillRect(dc, &rect, brush);
            DeleteObject(brush);
        }
    }
}

fn line(dc: HDC, color: u32, width: i32, x1: i32, y1: i32, x2: i32, y2: i32) {
    let pen = unsafe { CreatePen(PS_SOLID, width, color) };
    if pen.is_null() {
        return;
    }
    let old = unsafe { SelectObject(dc, pen) };
    unsafe {
        MoveToEx(dc, x1, y1, null_mut());
        LineTo(dc, x2, y2);
        SelectObject(dc, old);
        DeleteObject(pen);
    }
}

fn circle(dc: HDC, color: u32, x: i32, y: i32, radius: i32) {
    let brush = unsafe { CreateSolidBrush(color) };
    let pen = unsafe { CreatePen(PS_SOLID, 1, color) };
    if brush.is_null() || pen.is_null() {
        unsafe {
            // SAFETY: Delete only the successfully created local GDI objects.
            if !brush.is_null() {
                DeleteObject(brush);
            }
            if !pen.is_null() {
                DeleteObject(pen);
            }
        }
        return;
    }
    let old_brush = unsafe { SelectObject(dc, brush) };
    let old_pen = unsafe { SelectObject(dc, pen) };
    unsafe {
        Ellipse(dc, x - radius, y - radius, x + radius, y + radius);
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

fn font(dpi: u32, points: i32, weight: i32) -> HFONT {
    let variable = create_font(dpi, points, weight, "Segoe UI Variable Text");
    if variable.is_null() {
        create_font(dpi, points, weight, "Segoe UI")
    } else {
        variable
    }
}

fn create_font(dpi: u32, points: i32, weight: i32, face: &str) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            -scale(points, dpi),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            0,
            0,
            0,
            face.as_ptr(),
        )
    }
}

fn apply_theme(state: &WindowState) {
    let dark = effective_dark(state.pending_theme.get());
    let value: i32 = i32::from(dark);
    unsafe {
        // SAFETY: The attribute receives a correctly sized BOOL-compatible integer.
        DwmSetWindowAttribute(
            state.hwnd.get(),
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            (&value as *const i32).cast::<c_void>(),
            size_of::<i32>() as u32,
        );
    }
}

fn palette(theme: Theme) -> Palette {
    if effective_dark(theme) {
        Palette {
            background: rgb(17, 18, 20),
            card: rgb(28, 28, 30),
            card_outline: rgb(55, 56, 60),
            highlight: rgb(75, 76, 80),
            shadow: rgb(10, 10, 12),
            text: rgb(245, 245, 247),
            secondary: rgb(152, 152, 157),
            accent: rgb(10, 132, 255),
            track: rgb(58, 58, 60),
            edit: rgb(39, 39, 42),
            pill: rgb(45, 45, 48),
            pill_hover: rgb(55, 55, 59),
            pill_pressed: rgb(68, 68, 72),
            selected_outline: rgb(80, 166, 255),
            status_ok: rgb(48, 209, 88),
            status_warn: rgb(255, 159, 10),
        }
    } else {
        Palette {
            background: rgb(245, 245, 247),
            card: rgb(255, 255, 255),
            card_outline: rgb(229, 229, 234),
            highlight: rgb(255, 255, 255),
            shadow: rgb(225, 225, 230),
            text: rgb(29, 29, 31),
            secondary: rgb(110, 110, 115),
            accent: rgb(0, 122, 255),
            track: rgb(217, 217, 222),
            edit: rgb(250, 250, 252),
            pill: rgb(244, 244, 247),
            pill_hover: rgb(235, 235, 240),
            pill_pressed: rgb(221, 221, 228),
            selected_outline: rgb(80, 166, 255),
            status_ok: rgb(40, 180, 75),
            status_warn: rgb(230, 135, 0),
        }
    }
}

fn effective_dark(theme: Theme) -> bool {
    match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => {
            let color = unsafe { GetSysColor(COLOR_WINDOW) };
            let red = color & 0xff;
            let green = (color >> 8) & 0xff;
            let blue = (color >> 16) & 0xff;
            red * 299 + green * 587 + blue * 114 < 128_000
        }
    }
}

fn slider_hit(state: &WindowState, x: i32, y: i32) -> bool {
    let dpi = state.dpi.get();
    contains(SLIDER_RECT, (unscale(x, dpi), unscale(y, dpi)))
}

fn preset_rect(index: usize) -> (i32, i32, i32, i32) {
    (36 + index as i32 * 96, 402, 80, 34)
}

fn edit_rect(index: usize) -> (i32, i32, i32, i32) {
    (36 + index as i32 * 96, 507, 80, 30)
}

fn theme_rect(index: usize) -> (i32, i32, i32, i32) {
    (36 + index as i32 * 84, 569, 84, 32)
}

fn dpi_rect(rect: (i32, i32, i32, i32), dpi: u32) -> RECT {
    RECT {
        left: scale(rect.0, dpi),
        top: scale(rect.1, dpi),
        right: scale(rect.0 + rect.2, dpi),
        bottom: scale(rect.1 + rect.3, dpi),
    }
}

fn battery_detail(state: &logipeek::app::state::AppState) -> String {
    let level = match state.battery_level.as_ref() {
        Some(Level::Critical) => Some("Critical"),
        Some(Level::Low) => Some("Low"),
        Some(Level::Good) => Some("Good"),
        Some(Level::Full) => Some("Full"),
        Some(Level::Unknown(_)) => Some("Unknown"),
        None => None,
    };
    let charging = state.charging_text();
    match (level, charging) {
        (Some(level), "Status unavailable") => level.into(),
        (Some(level), charging) => format!("{level} · {charging}"),
        (None, charging) => charging.into(),
    }
}

fn app(state: &WindowState) -> &AppContext {
    unsafe {
        // SAFETY: AppContext is boxed before MainWindow and outlives WindowState and its HWND.
        &*state.app
    }
}

fn point_from_lparam(lparam: LPARAM) -> (i32, i32) {
    (lparam as i16 as i32, (lparam >> 16) as i16 as i32)
}

fn contains(rect: (i32, i32, i32, i32), point: (i32, i32)) -> bool {
    point.0 >= rect.0 && point.0 < rect.0 + rect.2 && point.1 >= rect.1 && point.1 < rect.1 + rect.3
}

fn invalidate(hwnd: HWND) {
    if !hwnd.is_null() {
        unsafe {
            // SAFETY: Invalidating schedules paint and does not retain the RECT pointer.
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

fn scale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * i64::from(dpi) + 48) / 96) as i32
}

fn unscale(value: i32, dpi: u32) -> i32 {
    ((i64::from(value) * 96 + i64::from(dpi) / 2) / i64::from(dpi.max(1))) as i32
}

fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
