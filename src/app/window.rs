use crate::tray::AppContext;
use logipeek::app::{
    settings::{Settings, Theme},
    slider,
    state::{DeviceStatus, OperationStatus},
};
use std::{
    cell::Cell,
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{null, null_mut},
    sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
        Gdi::{
            BeginPaint, COLOR_WINDOW, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
            DeleteObject, Ellipse, EndPaint, FillRect, GetSysColor, HDC, HFONT, InvalidateRect,
            LineTo, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject,
            SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
        },
    },
    UI::{
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem},
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_F5, VK_RETURN, VK_TAB},
        WindowsAndMessaging::{
            CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
            GWLP_USERDATA, GetClientRect, GetSystemMetrics, GetWindowLongPtrW, IDC_ARROW,
            LoadCursorW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_RESTORE, SW_SHOW,
            SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, ShowWindow, UnregisterClassW,
            WM_APP, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DPICHANGED,
            WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
            WM_NCDESTROY, WM_PAINT, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SYSCOLORCHANGE,
            WM_THEMECHANGED, WNDCLASSW, WS_CAPTION, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_OVERLAPPED,
            WS_SYSMENU,
        },
    },
};

const CLASS_NAME: &str = "LogiPeek.Settings.Window";
const CLIENT_WIDTH: i32 = 440;
const CLIENT_HEIGHT: i32 = 660;
const WM_REDRAW: u32 = WM_APP + 10;

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
}

#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    card: u32,
    text: u32,
    secondary: u32,
    accent: u32,
    track: u32,
    edit: u32,
}

struct Button<'a> {
    rect: (i32, i32, i32, i32),
    label: &'a str,
    enabled: bool,
    selected: bool,
}

impl MainWindow {
    pub(crate) fn create(instance: HINSTANCE, app: &AppContext) -> Result<Self, String> {
        let class_name = wide(CLASS_NAME);
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: unsafe {
                // SAFETY: IDC_ARROW is a system cursor with process-independent lifetime.
                LoadCursorW(null_mut(), IDC_ARROW)
            },
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if unsafe {
            // SAFETY: The class descriptor and class name stay valid through this call.
            RegisterClassW(&window_class)
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
            state.dpi.set(((wparam >> 16) as u32).max(96));
            let suggested = lparam as *const RECT;
            if !suggested.is_null() {
                let rect = unsafe {
                    // SAFETY: WM_DPICHANGED supplies a valid suggested RECT for this call.
                    *suggested
                };
                unsafe {
                    // SAFETY: The suggested rectangle is copied before this reentrant window call.
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        0,
                    );
                }
            }
            invalidate(hwnd);
            return 0;
        }
        WM_LBUTTONDOWN => {
            let point = point_from_lparam(lparam);
            if slider_hit(state, point.0, point.1) {
                begin_slider(state, point.0);
            }
            return 0;
        }
        WM_MOUSEMOVE => {
            if state.dragging.get() {
                preview_slider(state, point_from_lparam(lparam).0);
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
                handle_click(state, x, y);
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

fn handle_click(state: &WindowState, x: i32, y: i32) {
    let dpi = state.dpi.get();
    let logical = (unscale(x, dpi), unscale(y, dpi));
    if contains((338, 20, 78, 32), logical) {
        app(state).refresh();
        return;
    }
    let snapshot = app(state).snapshot();
    for index in 0..4 {
        if contains((31 + index as i32 * 99, 500, 80, 30), logical) {
            state.editing.set(Some(index));
            unsafe {
                // SAFETY: Focusing the owned top-level window enables direct numeric editing.
                SetFocus(state.hwnd.get());
            }
            invalidate(state.hwnd.get());
            return;
        }
    }
    for (index, preset) in snapshot.presets().into_iter().enumerate() {
        if contains((30 + index as i32 * 99, 392, 80, 34), logical) && preset.enabled {
            app(state).submit_dpi(preset.dpi);
            return;
        }
    }
    for (index, theme) in [Theme::System, Theme::Light, Theme::Dark]
        .into_iter()
        .enumerate()
    {
        if contains((31 + index as i32 * 126, 552, 112, 32), logical) {
            state.pending_theme.set(theme);
            apply_theme(state);
            invalidate(state.hwnd.get());
            return;
        }
    }
    if contains((290, 598, 126, 34), logical) {
        save_from_controls(state);
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
    let left = scale(45, dpi);
    let width = scale(350, dpi).max(1);
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
            app(state).set_notice("Each preset must be a positive integer up to 65535");
            return;
        };
        if value == 0 {
            app(state).set_notice("Each preset must be a positive integer up to 65535");
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

    let title = font(dpi, 25, 600);
    let heading = font(dpi, 12, 600);
    let body = font(dpi, 15, 400);
    let value_font = font(dpi, 23, 600);
    draw_text(
        dc,
        title,
        palette.text,
        scale(22, dpi),
        scale(20, dpi),
        "LogiPeek",
    );
    draw_button(
        dc,
        body,
        palette,
        dpi,
        Button {
            rect: (338, 20, 78, 32),
            label: "Refresh",
            enabled: true,
            selected: false,
        },
    );

    let device_name = if snapshot.status == DeviceStatus::Single {
        "Logitech HID++ Device"
    } else {
        "Logitech Mouse"
    };
    draw_text(
        dc,
        body,
        palette.text,
        scale(22, dpi),
        scale(67, dpi),
        device_name,
    );
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(22, dpi),
        scale(89, dpi),
        snapshot.connection_text(),
    );

    draw_card(dc, palette, dpi, (20, 118, 400, 104));
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(36, dpi),
        scale(134, dpi),
        "BATTERY",
    );
    let battery = snapshot.battery_value_text();
    draw_text(
        dc,
        value_font,
        palette.text,
        scale(335, dpi),
        scale(130, dpi),
        &battery,
    );
    if let Some(percent) = snapshot.battery_percent {
        draw_progress(dc, palette, dpi, percent);
    }
    draw_text(
        dc,
        body,
        palette.secondary,
        scale(36, dpi),
        scale(190, dpi),
        snapshot.charging_text(),
    );

    draw_card(dc, palette, dpi, (20, 236, 400, 208));
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(36, dpi),
        scale(252, dpi),
        "DPI",
    );
    let displayed =
        if state.dragging.get() || matches!(snapshot.operation, OperationStatus::Applying(_)) {
            state.preview.get().or(snapshot.current_dpi)
        } else {
            snapshot.current_dpi
        };
    draw_text(
        dc,
        value_font,
        palette.text,
        scale(335, dpi),
        scale(248, dpi),
        &displayed.map_or_else(|| "—".into(), |value| value.to_string()),
    );
    draw_slider(dc, &snapshot, palette, dpi, displayed);
    for (index, preset) in snapshot.presets().into_iter().enumerate() {
        let label = preset.dpi.to_string();
        draw_button(
            dc,
            body,
            palette,
            dpi,
            Button {
                rect: (30 + index as i32 * 99, 392, 80, 34),
                label: &label,
                enabled: preset.enabled,
                selected: preset.checked,
            },
        );
    }
    let operation = snapshot.operation_text();
    if !operation.is_empty() {
        draw_text(
            dc,
            heading,
            palette.secondary,
            scale(36, dpi),
            scale(365, dpi),
            &operation,
        );
    }

    draw_card(dc, palette, dpi, (20, 460, 400, 184));
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(36, dpi),
        scale(474, dpi),
        "DPI PRESETS",
    );
    let drafts = state
        .drafts
        .lock()
        .map(|drafts| drafts.clone())
        .unwrap_or_else(|_| snapshot.presets.map(|value| value.to_string()));
    for (index, label) in drafts.iter().enumerate() {
        draw_button(
            dc,
            body,
            Palette {
                track: palette.edit,
                ..palette
            },
            dpi,
            Button {
                rect: (31 + index as i32 * 99, 500, 80, 30),
                label,
                enabled: true,
                selected: state.editing.get() == Some(index),
            },
        );
    }
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(36, dpi),
        scale(536, dpi),
        "APPEARANCE",
    );
    for (index, (theme, label)) in [
        (Theme::System, "System"),
        (Theme::Light, "Light"),
        (Theme::Dark, "Dark"),
    ]
    .into_iter()
    .enumerate()
    {
        draw_button(
            dc,
            body,
            palette,
            dpi,
            Button {
                rect: (31 + index as i32 * 126, 552, 112, 32),
                label,
                enabled: true,
                selected: state.pending_theme.get() == theme,
            },
        );
    }
    draw_button(
        dc,
        body,
        palette,
        dpi,
        Button {
            rect: (290, 598, 126, 34),
            label: "Save Settings",
            enabled: true,
            selected: false,
        },
    );
    if let Some(notice) = snapshot.settings_notice.as_deref() {
        draw_text(
            dc,
            heading,
            palette.secondary,
            scale(36, dpi),
            scale(607, dpi),
            notice,
        );
    }
    draw_text(
        dc,
        heading,
        palette.secondary,
        scale(24, dpi),
        scale(646, dpi),
        "Closing this window keeps LogiPeek running in the tray.",
    );

    for object in [title, heading, body, value_font] {
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
    let left = scale(45, dpi);
    let right = scale(395, dpi);
    let y = scale(323, dpi);
    line(dc, palette.track, scale(4, dpi).max(1), left, y, right, y);
    if let Some(values) = snapshot.supported_dpi.as_ref() {
        if let Some(value) = displayed
            && let Some(position) = slider::position_of(values, value, right - left)
        {
            line(
                dc,
                palette.accent,
                scale(4, dpi).max(1),
                left,
                y,
                left + position,
                y,
            );
            circle(dc, palette.accent, left + position, y, scale(8, dpi));
        }
        if let Some((minimum, maximum)) = slider::endpoints(values) {
            let small = font(dpi, 11, 400);
            draw_text(
                dc,
                small,
                palette.secondary,
                left,
                scale(340, dpi),
                &minimum.to_string(),
            );
            draw_text(
                dc,
                small,
                palette.secondary,
                scale(350, dpi),
                scale(340, dpi),
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
            scale(340, dpi),
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
        left: scale(36, dpi),
        top: scale(169, dpi),
        right: scale(404, dpi),
        bottom: scale(178, dpi),
    };
    fill(dc, track, palette.track);
    let filled = RECT {
        right: track.left + (track.right - track.left) * i32::from(percent.min(100)) / 100,
        ..track
    };
    fill(dc, filled, palette.accent);
}

fn draw_card(dc: HDC, palette: Palette, dpi: u32, rect: (i32, i32, i32, i32)) {
    let brush = unsafe { CreateSolidBrush(palette.card) };
    let pen = unsafe { CreatePen(PS_SOLID, 1, palette.card) };
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
        RoundRect(
            dc,
            scale(rect.0, dpi),
            scale(rect.1, dpi),
            scale(rect.0 + rect.2, dpi),
            scale(rect.1 + rect.3, dpi),
            scale(18, dpi),
            scale(18, dpi),
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

fn draw_button(dc: HDC, font: HFONT, palette: Palette, dpi: u32, button: Button<'_>) {
    let background = if button.selected {
        palette.accent
    } else {
        palette.track
    };
    draw_card(
        dc,
        Palette {
            card: background,
            ..palette
        },
        dpi,
        button.rect,
    );
    let color = if button.selected {
        rgb(255, 255, 255)
    } else if button.enabled {
        palette.text
    } else {
        palette.secondary
    };
    draw_text(
        dc,
        font,
        color,
        scale(button.rect.0 + 11, dpi),
        scale(button.rect.1 + 8, dpi),
        button.label,
    );
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
    let face = wide("Segoe UI");
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
            background: rgb(28, 28, 30),
            card: rgb(44, 44, 46),
            text: rgb(242, 242, 247),
            secondary: rgb(174, 174, 178),
            accent: rgb(10, 132, 255),
            track: rgb(72, 72, 74),
            edit: rgb(58, 58, 60),
        }
    } else {
        Palette {
            background: rgb(245, 245, 247),
            card: rgb(255, 255, 255),
            text: rgb(29, 29, 31),
            secondary: rgb(110, 110, 115),
            accent: rgb(0, 122, 255),
            track: rgb(209, 209, 214),
            edit: rgb(255, 255, 255),
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
    x >= scale(35, dpi) && x <= scale(405, dpi) && y >= scale(302, dpi) && y <= scale(342, dpi)
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
