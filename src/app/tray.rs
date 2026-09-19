use logipeek::app::{
    state::AppState,
    worker::{Command, Worker},
};
use std::{
    cell::Cell,
    ffi::c_void,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{null, null_mut},
    sync::{Arc, Mutex},
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
        POINT, WPARAM,
    },
    System::{Console::FreeConsole, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{
        Shell::{
            NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NIM_SETFOCUS, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
            DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW,
            GetWindowLongPtrW, HICON, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
            MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
            SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
            TrackPopupMenu, TranslateMessage, UnregisterClassW, WM_APP, WM_CLOSE, WM_COMMAND,
            WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP, WM_NCDESTROY, WM_NULL, WM_RBUTTONUP,
            WNDCLASSW, WS_EX_TOOLWINDOW,
        },
    },
};

const CLASS_NAME: &str = "LogiPeek.Tray.Window";
const MUTEX_NAME: &str = "Local\\LogiPeek.Tray.5DF85F89-74D5-4F19-AEEC-44483F2D35EA";
const ICON_ID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_STATE_UPDATED: u32 = WM_APP + 2;
const PRESET_FIRST: u32 = 100;
const REFRESH_ID: u32 = 200;
const EXIT_ID: u32 = 201;

pub fn run() -> Result<(), String> {
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

    let icon = match create_icon(instance) {
        Ok(icon) => icon,
        Err(error) => {
            unsafe {
                // SAFETY: The window and class were created by this thread and are not in use yet.
                DestroyWindow(hwnd);
                UnregisterClassW(class_name.as_ptr(), instance);
            }
            return Err(error);
        }
    };
    let state = Arc::new(Mutex::new(AppState::default()));
    let mut context = Box::new(TrayContext {
        state: Arc::clone(&state),
        worker: None,
        hwnd,
        icon,
        icon_added: Cell::new(false),
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
            (&mut *context as *mut TrayContext) as isize,
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

    context.remove_icon();
    if let Some(worker) = context.worker.take() {
        worker.shutdown();
    }
    unsafe {
        // SAFETY: The icon was created by CreateIcon and remains owned by this process.
        DestroyIcon(context.icon);
        UnregisterClassW(class_name.as_ptr(), instance);
    }
    drop(mutex);
    if message_loop_failed {
        Err("The Windows message loop failed".into())
    } else {
        Ok(())
    }
}

struct TrayContext {
    state: Arc<Mutex<AppState>>,
    worker: Option<Worker>,
    hwnd: HWND,
    icon: HICON,
    icon_added: Cell<bool>,
    taskbar_created: u32,
}

impl TrayContext {
    fn add_icon(&self) -> bool {
        let mut data = self.icon_data(NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icon;
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
        let Some(worker) = &self.worker else {
            return;
        };
        if command == REFRESH_ID {
            worker.send(Command::RefreshAll);
        } else if command == EXIT_ID {
            unsafe {
                // SAFETY: hwnd belongs to the UI thread; WM_CLOSE performs normal cleanup.
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            }
        } else if (PRESET_FIRST..PRESET_FIRST + 4).contains(&command) {
            let preset = state.presets()[(command - PRESET_FIRST) as usize];
            if preset.enabled {
                worker.send(Command::SetDpi(preset.dpi));
            }
        }
    }

    fn snapshot(&self) -> AppState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
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
            DestroyIcon(self.icon);
            UnregisterClassW(class_name, instance);
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
    } as *mut TrayContext;
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
                if matches!(event, WM_CONTEXTMENU | WM_RBUTTONUP | WM_LBUTTONUP) {
                    context.show_menu();
                }
                return 0;
            }
            WM_STATE_UPDATED => {
                context.update_tooltip();
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
                    // SAFETY: hwnd is the live hidden window being closed.
                    DestroyWindow(hwnd);
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

fn create_icon(instance: HINSTANCE) -> Result<HICON, String> {
    let and_mask = [0xffu8; 128];
    let mut xor_mask = [0u8; 128];
    for y in 4..27 {
        for x in 8..24 {
            let dx = x as i32 * 2 - 31;
            let dy = y as i32 - 15;
            let edge = (dx * dx + dy * dy * 2) >= 205 && (dx * dx + dy * dy * 2) <= 285;
            if edge || (x == 15 && (5..12).contains(&y)) {
                set_icon_bit(&mut xor_mask, x, y);
            }
        }
    }
    let icon = unsafe {
        // SAFETY: Both masks contain exactly 32 rows of 32 one-bit pixels.
        CreateIcon(instance, 32, 32, 1, 1, and_mask.as_ptr(), xor_mask.as_ptr())
    };
    if icon.is_null() {
        Err("Could not create the LogiPeek tray icon".into())
    } else {
        Ok(icon)
    }
}

fn set_icon_bit(mask: &mut [u8; 128], x: usize, y: usize) {
    mask[y * 4 + x / 8] |= 0x80 >> (x % 8);
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
