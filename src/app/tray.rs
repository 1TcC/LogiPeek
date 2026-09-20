use logipeek::app::{
    settings::Settings,
    state::{AppState, OperationStatus},
    worker::{Command, Worker},
};
use std::{
    cell::Cell,
    env,
    ffi::c_void,
    mem::size_of,
    os::windows::ffi::OsStrExt,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::{Arc, Mutex},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
        POINT, WPARAM,
    },
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject,
    },
    Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    System::{Console::FreeConsole, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        Input::Ime::ImmDisableIME,
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NIM_SETFOCUS, NIM_SETVERSION, NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
            DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos,
            GetMessageW, GetWindowLongPtrW, HICON, ICONINFO, MF_CHECKED, MF_DISABLED, MF_GRAYED,
            MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
            RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, UnregisterClassW,
            WM_APP, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK,
            WM_NCDESTROY, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW,
        },
    },
};

const CLASS_NAME: &str = "LogiPeek.Tray.Window";
const MUTEX_NAME: &str = "Local\\LogiPeek.Tray.5DF85F89-74D5-4F19-AEEC-44483F2D35EA";
const ICON_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_STATE_UPDATED: u32 = WM_APP + 2;
const OPEN_ID: u32 = 90;
const PRESET_FIRST: u32 = 100;
const REFRESH_ID: u32 = 200;
const EXIT_ID: u32 = 201;

pub fn run() -> Result<(), String> {
    unsafe {
        // SAFETY: This runs before either application window is created. A false return can
        // mean awareness was already fixed by the host or manifest, so graceful fallback is safe.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // SAFETY: LogiPeek accepts ASCII digits only and disables IME for this process's UI
        // threads; this does not change the user's system input-method configuration.
        ImmDisableIME(u32::MAX);
    }
    let mutex = SingleInstance::acquire()?;
    let Some(mutex) = mutex else {
        return Ok(());
    };
    let class_name = wide(CLASS_NAME);
    let window_name = wide("LogiPeek");
    let taskbar_created_name = wide("TaskbarCreated");
    let instance = unsafe {
        // SAFETY: A null module name asks Windows for the current process module.
        GetModuleHandleW(null())
    };
    if instance.is_null() {
        return Err("Could not resolve the LogiPeek module handle".into());
    }
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    if unsafe {
        // SAFETY: The class strings remain alive until after UnregisterClassW.
        RegisterClassW(&window_class)
    } == 0
    {
        return Err("Could not register the LogiPeek tray window class".into());
    }

    let hwnd = unsafe {
        // SAFETY: Parameters describe an invisible top-level window owned by this thread.
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        unsafe {
            // SAFETY: The class was registered by this thread and no window was created.
            UnregisterClassW(class_name.as_ptr(), instance);
        }
        return Err("Could not create the LogiPeek tray window".into());
    }

    let icons = match AppIcons::create(instance) {
        Ok(icons) => icons,
        Err(error) => {
            unsafe {
                // SAFETY: The window and class were created by this thread and are not in use yet.
                DestroyWindow(hwnd);
                UnregisterClassW(class_name.as_ptr(), instance);
            }
            return Err(error);
        }
    };
    let settings_path = settings_path();
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();
    let mut initial_state = AppState::default();
    initial_state.apply_settings(&settings);
    let state = Arc::new(Mutex::new(initial_state));
    let mut context = Box::new(AppContext {
        state: Arc::clone(&state),
        worker: None,
        hwnd,
        main_hwnd: Cell::new(null_mut()),
        icons,
        icon_added: Cell::new(false),
        settings_path,
        taskbar_created: unsafe {
            // SAFETY: The string is NUL-terminated and valid for this call.
            RegisterWindowMessageW(taskbar_created_name.as_ptr())
        },
    });
    unsafe {
        // SAFETY: Context is boxed and remains at a stable address through the message loop.
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *context as *mut AppContext) as isize,
        );
    }
    if !context.add_icon() {
        context.cleanup_window(class_name.as_ptr(), instance);
        return Err("Could not add the LogiPeek notification icon".into());
    }

    let notify_hwnd = hwnd as usize;
    context.worker = match Worker::spawn(state, move || unsafe {
        // SAFETY: A stale or destroyed HWND makes PostMessageW fail harmlessly; no pointer is sent.
        PostMessageW(notify_hwnd as HWND, WM_STATE_UPDATED, 0, 0);
    }) {
        Ok(worker) => Some(worker),
        Err(error) => {
            context.cleanup_window(class_name.as_ptr(), instance);
            return Err(format!("Could not start HID worker: {error}"));
        }
    };
    let main_window = match crate::window::MainWindow::create(
        instance,
        &context,
        context.icons.normal,
        context.icons.small,
    ) {
        Ok(window) => window,
        Err(error) => {
            if let Some(worker) = context.worker.take() {
                worker.shutdown();
            }
            context.cleanup_window(class_name.as_ptr(), instance);
            return Err(error);
        }
    };
    main_window.show();
    unsafe {
        // SAFETY: Detaching affects only this successfully initialized no-argument tray process.
        FreeConsole();
    }

    let mut message = MSG::default();
    let message_loop_failed = loop {
        let result = unsafe {
            // SAFETY: message points to writable storage for the duration of the call.
            GetMessageW(&mut message, null_mut(), 0, 0)
        };
        if result > 0 {
            unsafe {
                // SAFETY: The message came from this thread's Windows message queue.
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        } else {
            break result < 0;
        }
    };

    main_window.destroy();
    context.remove_icon();
    if let Some(worker) = context.worker.take() {
        worker.shutdown();
    }
    unsafe {
        // SAFETY: The UI thread owns both resources; DestroyWindow is harmless if destruction
        // already completed, and both icons were created by CreateIconIndirect.
        DestroyWindow(context.hwnd);
        UnregisterClassW(class_name.as_ptr(), instance);
    }
    context.icons.destroy();
    drop(mutex);
    if message_loop_failed {
        Err("The Windows message loop failed".into())
    } else {
        Ok(())
    }
}

pub(crate) struct AppContext {
    state: Arc<Mutex<AppState>>,
    worker: Option<Worker>,
    hwnd: HWND,
    main_hwnd: Cell<HWND>,
    icons: AppIcons,
    icon_added: Cell<bool>,
    settings_path: Option<PathBuf>,
    taskbar_created: u32,
}

impl AppContext {
    fn add_icon(&self) -> bool {
        let mut data = self.icon_data(NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icons.small;
        copy_wide(&self.snapshot().tooltip(), &mut data.szTip);
        let added = unsafe {
            // SAFETY: data has the documented size, valid window, owned icon, and fixed buffers.
            Shell_NotifyIconW(NIM_ADD, &data)
        } != 0;
        if !added {
            return false;
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let versioned = unsafe {
            // SAFETY: The icon was just registered with the same window and ID.
            Shell_NotifyIconW(NIM_SETVERSION, &data)
        } != 0;
        if !versioned {
            unsafe {
                // SAFETY: Roll back the registration created above.
                Shell_NotifyIconW(NIM_DELETE, &data);
            }
            return false;
        }
        self.icon_added.set(true);
        true
    }

    fn update_tooltip(&self) {
        if !self.icon_added.get() {
            return;
        }
        let mut data = self.icon_data(NIF_TIP | NIF_SHOWTIP);
        copy_wide(&self.snapshot().tooltip(), &mut data.szTip);
        unsafe {
            // SAFETY: The tray icon is registered and data owns a terminated tooltip buffer.
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    fn remove_icon(&self) {
        if self.icon_added.get() {
            let data = self.icon_data(0);
            unsafe {
                // SAFETY: The tray icon uses this window and ID; deletion is idempotently guarded.
                Shell_NotifyIconW(NIM_DELETE, &data);
            }
            self.icon_added.set(false);
        }
    }

    fn show_menu(&self) {
        let menu = unsafe {
            // SAFETY: CreatePopupMenu has no inputs and returns an owned menu handle.
            CreatePopupMenu()
        };
        if menu.is_null() {
            return;
        }
        let state = self.snapshot();
        append(menu, MF_STRING | MF_DISABLED, 0, "LogiPeek");
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING | MF_DISABLED, 0, &state.battery_text());
        append(menu, MF_STRING | MF_DISABLED, 0, &state.dpi_text());
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, OPEN_ID as usize, "Open LogiPeek");
        append(menu, MF_SEPARATOR, 0, "");
        for (offset, preset) in state.presets().into_iter().enumerate() {
            let mut flags = MF_STRING;
            if !preset.enabled {
                flags |= MF_DISABLED | MF_GRAYED;
            }
            if preset.checked {
                flags |= MF_CHECKED;
            }
            append(
                menu,
                flags,
                PRESET_FIRST as usize + offset,
                &format!("{} DPI", preset.dpi),
            );
        }
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, REFRESH_ID as usize, "Refresh");
        append(menu, MF_SEPARATOR, 0, "");
        append(menu, MF_STRING, EXIT_ID as usize, "Exit");

        let mut point = POINT::default();
        let command = unsafe {
            // SAFETY: menu and hwnd are valid; GetCursorPos writes a POINT and TrackPopupMenu is synchronous.
            GetCursorPos(&mut point);
            SetForegroundWindow(self.hwnd);
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                point.x,
                point.y,
                0,
                self.hwnd,
                null(),
            ) as u32
        };
        unsafe {
            // SAFETY: TrackPopupMenu has returned, so the owned popup menu can be destroyed.
            DestroyMenu(menu);
            PostMessageW(self.hwnd, WM_NULL, 0, 0);
            let data = self.icon_data(0);
            Shell_NotifyIconW(NIM_SETFOCUS, &data);
        }
        self.dispatch_command(command, &state);
    }

    fn dispatch_command(&self, command: u32, state: &AppState) {
        if command == OPEN_ID {
            crate::window::show(self.main_hwnd.get());
        } else if command == REFRESH_ID {
            self.refresh();
        } else if command == EXIT_ID {
            unsafe {
                // SAFETY: hwnd belongs to the UI thread; WM_CLOSE performs normal cleanup.
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            }
        } else if (PRESET_FIRST..PRESET_FIRST + 4).contains(&command) {
            let preset = state.presets()[(command - PRESET_FIRST) as usize];
            if preset.enabled {
                self.submit_dpi(preset.dpi);
            }
        }
    }

    pub(crate) fn snapshot(&self) -> AppState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    pub(crate) fn set_main_hwnd(&self, hwnd: HWND) {
        self.main_hwnd.set(hwnd);
    }

    pub(crate) fn refresh(&self) {
        let sent = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.send(Command::RefreshAll));
        if !sent && let Ok(mut state) = self.state.lock() {
            state.operation = OperationStatus::Failed("HID worker is busy; try again".into());
        }
        crate::window::state_changed(self.main_hwnd.get());
    }

    pub(crate) fn submit_dpi(&self, value: u16) {
        if let Ok(mut state) = self.state.lock() {
            state.operation = OperationStatus::Applying(value);
        }
        let sent = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.send(Command::SetDpi(value)));
        if !sent && let Ok(mut state) = self.state.lock() {
            state.operation = OperationStatus::Failed("HID worker is busy; try again".into());
        }
        crate::window::state_changed(self.main_hwnd.get());
    }

    pub(crate) fn save_settings(&self, settings: Settings) {
        let result = self
            .settings_path
            .as_deref()
            .ok_or_else(|| "LOCALAPPDATA is unavailable".to_string())
            .and_then(|path| save_settings_atomic(&settings, path));
        if let Ok(mut state) = self.state.lock() {
            state.apply_settings(&settings);
            state.settings_notice = Some(match result {
                Ok(()) => "Settings saved".into(),
                Err(_) => "Settings could not be saved".into(),
            });
        }
        crate::window::state_changed(self.main_hwnd.get());
    }

    pub(crate) fn set_notice(&self, notice: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.settings_notice = Some(notice.into());
        }
        crate::window::state_changed(self.main_hwnd.get());
    }

    fn icon_data(&self, flags: u32) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: ICON_ID,
            uFlags: flags,
            ..Default::default()
        }
    }

    fn cleanup_window(&self, class_name: *const u16, instance: HINSTANCE) {
        self.remove_icon();
        unsafe {
            // SAFETY: Resources were created by this thread and have not been released.
            DestroyWindow(self.hwnd);
            UnregisterClassW(class_name, instance);
        }
        self.icons.destroy();
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    catch_unwind(AssertUnwindSafe(|| unsafe {
        // SAFETY: The inner handler validates the stored pointer before dereferencing it.
        window_proc_inner(hwnd, message, wparam, lparam)
    }))
    .unwrap_or_else(|_| unsafe {
        // SAFETY: Falling back to the default procedure prevents unwinding across the FFI boundary.
        DefWindowProcW(hwnd, message, wparam, lparam)
    })
}

unsafe fn window_proc_inner(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let pointer = unsafe {
        // SAFETY: Reading GWLP_USERDATA does not dereference the stored value.
        GetWindowLongPtrW(hwnd, GWLP_USERDATA)
    } as *mut AppContext;
    if !pointer.is_null() {
        let context = unsafe {
            // SAFETY: run() stores a stable Box pointer before processing application messages.
            // Shared access remains valid across reentrant Win32 calls; mutable UI state uses Cell.
            &*pointer
        };
        if message == context.taskbar_created {
            context.icon_added.set(false);
            context.add_icon();
            return 0;
        }
        match message {
            WM_TRAY => {
                let event = lparam as u32 & 0xffff;
                if matches!(event, WM_CONTEXTMENU | WM_RBUTTONUP) {
                    context.show_menu();
                } else if matches!(event, WM_LBUTTONDBLCLK | NIN_SELECT) {
                    crate::window::show(context.main_hwnd.get());
                }
                return 0;
            }
            WM_STATE_UPDATED => {
                context.update_tooltip();
                crate::window::state_changed(context.main_hwnd.get());
                return 0;
            }
            WM_COMMAND => {
                let command = wparam as u32 & 0xffff;
                let state = context.snapshot();
                context.dispatch_command(command, &state);
                return 0;
            }
            WM_CLOSE => {
                context.remove_icon();
                unsafe {
                    // SAFETY: run() joins the worker before it destroys the notification window.
                    PostQuitMessage(0);
                }
                return 0;
            }
            WM_NCDESTROY => unsafe {
                // SAFETY: Stop future callbacks from observing the soon-to-be-released context.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            },
            WM_DESTROY => {
                unsafe {
                    // SAFETY: Posting quit is the normal end of this UI thread's message loop.
                    PostQuitMessage(0);
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe {
        // SAFETY: Unhandled messages are delegated to the Windows default procedure.
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

fn append(menu: *mut c_void, flags: u32, id: usize, text: &str) {
    let text = wide(text);
    unsafe {
        // SAFETY: menu is live during menu construction and text is NUL-terminated for the call.
        AppendMenuW(menu, flags, id, text.as_ptr());
    }
}

pub(crate) struct AppIcons {
    pub(crate) normal: HICON,
    pub(crate) small: HICON,
}

impl AppIcons {
    fn create(instance: HINSTANCE) -> Result<Self, String> {
        let normal = create_icon(instance, 32)?;
        let small = match create_icon(instance, 16) {
            Ok(icon) => icon,
            Err(error) => {
                unsafe {
                    // SAFETY: normal was created above and has not been shared yet.
                    DestroyIcon(normal);
                }
                return Err(error);
            }
        };
        Ok(Self { normal, small })
    }

    fn destroy(&self) {
        unsafe {
            // SAFETY: The two distinct handles are owned by AppIcons and destroyed after every
            // window class and tray registration that references them has been released.
            DestroyIcon(self.small);
            DestroyIcon(self.normal);
        }
    }
}

fn create_icon(_instance: HINSTANCE, size: usize) -> Result<HICON, String> {
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = null_mut();
    let color = unsafe {
        // SAFETY: bitmap describes a top-down 32-bit DIB and bits receives its pixel storage.
        CreateDIBSection(
            null_mut(),
            &bitmap,
            DIB_RGB_COLORS,
            &mut bits,
            null_mut(),
            0,
        )
    };
    if color.is_null() || bits.is_null() {
        if !color.is_null() {
            unsafe {
                // SAFETY: color is an owned DIB handle returned by CreateDIBSection above.
                DeleteObject(color);
            }
        }
        return Err(format!(
            "Could not create the {size}px LogiPeek color bitmap"
        ));
    }
    let pixels = unsafe {
        // SAFETY: CreateDIBSection allocated exactly size * size 32-bit pixels above.
        std::slice::from_raw_parts_mut(bits.cast::<u32>(), size * size)
    };
    pixels.fill(0);
    for y in 0..size {
        for x in 0..size {
            let dx = ((x * 2 + 1) as i32 * 32 / size as i32) - 32;
            let dy = ((y * 2 + 1) as i32 * 32 / size as i32) - 32;
            if dx * dx + dy * dy <= 29 * 29 {
                pixels[y * size + x] = 0xff00_7aff;
            }
            let mouse = dx * dx * 3 + dy * dy;
            let edge = (330..=570).contains(&mouse) && (-23..=23).contains(&dy);
            let wheel = dx.abs() <= (48 / size as i32).max(2) && (-18..=-5).contains(&dy);
            if edge || wheel {
                pixels[y * size + x] = 0xffff_ffff;
            }
        }
    }
    let mask_stride = size.div_ceil(16) * 2;
    let mask_bits = vec![0u8; mask_stride * size];
    let mask = unsafe {
        // SAFETY: mask_bits holds word-aligned one-bit scanlines for the requested dimensions.
        CreateBitmap(
            size as i32,
            size as i32,
            1,
            1,
            mask_bits.as_ptr().cast::<c_void>(),
        )
    };
    if mask.is_null() {
        unsafe {
            DeleteObject(color);
        }
        return Err(format!("Could not create the {size}px LogiPeek mask"));
    }
    let info = ICONINFO {
        fIcon: 1,
        hbmMask: mask,
        hbmColor: color,
        ..Default::default()
    };
    let icon = unsafe {
        // SAFETY: ICONINFO refers to live mask and color bitmaps for the duration of the call.
        let icon = CreateIconIndirect(&info);
        DeleteObject(mask);
        DeleteObject(color);
        icon
    };
    if icon.is_null() {
        Err(format!("Could not create the {size}px LogiPeek icon"))
    } else {
        Ok(icon)
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn copy_wide<const N: usize>(value: &str, destination: &mut [u16; N]) {
    destination.fill(0);
    let mut offset = 0;
    for character in value.chars() {
        let mut encoded = [0u16; 2];
        let units = character.encode_utf16(&mut encoded);
        if offset + units.len() >= N {
            break;
        }
        destination[offset..offset + units.len()].copy_from_slice(units);
        offset += units.len();
    }
}

fn settings_path() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|directory| directory.join("LogiPeek").join("settings.ini"))
}

fn save_settings_atomic(settings: &Settings, path: &Path) -> Result<(), String> {
    let temporary = settings
        .write_temporary(path)
        .map_err(|error| error.to_string())?;
    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let moved = unsafe {
        // SAFETY: Both paths are NUL-terminated UTF-16 buffers and refer to files on the same volume.
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } != 0;
    if moved {
        Ok(())
    } else {
        let _ = std::fs::remove_file(temporary);
        Err("atomic settings replacement failed".into())
    }
}

struct SingleInstance(HANDLE);

impl SingleInstance {
    fn acquire() -> Result<Option<Self>, String> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe {
            // SAFETY: Security attributes are omitted and name is a valid NUL-terminated UTF-16 string.
            CreateMutexW(null(), 0, name.as_ptr())
        };
        if handle.is_null() {
            return Err("Could not create the LogiPeek single-instance mutex".into());
        }
        let already_exists = unsafe {
            // SAFETY: GetLastError is read immediately after CreateMutexW as required.
            GetLastError()
        } == ERROR_ALREADY_EXISTS;
        if already_exists {
            unsafe {
                // SAFETY: CreateMutexW returned an owned handle even when the object already existed.
                CloseHandle(handle);
            }
            Ok(None)
        } else {
            Ok(Some(Self(handle)))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: This handle is owned by SingleInstance and closed exactly once.
            CloseHandle(self.0);
        }
    }
}
