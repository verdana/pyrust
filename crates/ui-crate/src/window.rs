//! Win32 + GDI candidate window for the TSF IME.
//!
//! Lightweight, zero-dependency (beyond `windows` crate) candidate list.
//! Uses WS_EX_NOACTIVATE to prevent focus stealing.

use std::io::Write;
use std::sync::Mutex;
use std::thread;

use crossbeam_channel::{Receiver, Sender};

use crate::candidate_window::CandidateWindow;
use crate::{UiAction, UiUpdate};
use yas_config::UiConfig;

fn ui_log(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("C:\\Users\\Verdana\\pyrust_tsf.log")
    {
        let _ = writeln!(f, "[ui] {msg}");
        let _ = f.flush();
    }
}

// ── Win32 imports ───────────────────────────────────────────────────────

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM, COLORREF, HINSTANCE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, GetTextExtentPoint32W,
    Rectangle, SelectObject, SetBkMode, SetTextColor, TextOutW, HBRUSH, HFONT, PAINTSTRUCT,
    TRANSPARENT, PS_SOLID, CreatePen, MoveToEx, LineTo, GetDC, ReleaseDC,
    FONT_CHARSET, FONT_OUTPUT_PRECISION, FONT_CLIP_PRECISION, FONT_QUALITY,
};

use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCaretPos, GetForegroundWindow,
    GetGUIThreadInfo, GetMessageW, GetSystemMetrics, GUITHREADINFO,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetWindowPos, ShowWindow,
    TranslateMessage, DispatchMessageW,
    HWND_TOPMOST, MSG, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    WM_DESTROY, WM_LBUTTONDOWN, WM_PAINT, WM_USER, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    CS_HREDRAW, CS_VREDRAW, SM_CXSCREEN, SM_CYSCREEN, IDC_ARROW, LoadCursorW,
    GetClientRect,
};
use windows::Win32::Graphics::Gdi::InvalidateRect;

// ── Custom message ──────────────────────────────────────────────────────

const WM_CANDIDATE_UPDATE: u32 = WM_USER + 1;

// ── Global state ────────────────────────────────────────────────────────

struct UiState {
    window: CandidateWindow,
    font: HFONT,
    pinyin_color: COLORREF,
    text_color: COLORREF,
    index_color: COLORREF,
    bg_brush: HBRUSH,
    hover_brush: HBRUSH,
    font_size: i32,
    action_tx: Sender<UiAction>,
    /// Last known caret screen position (smoothed to avoid jitter)
    last_caret_x: i32,
    last_caret_y: i32,
}

// SAFETY: Win32 GDI handles are safe to send between threads as long as
// they are used from one thread at a time (protected by our Mutex).
unsafe impl Send for UiState {}

/// Global UI state shared between the update thread and WndProc thread.
static UI_STATE: Mutex<Option<UiState>> = Mutex::new(None);

// ── Public API ──────────────────────────────────────────────────────────

/// Run the candidate window. Blocks until the channel closes.
pub fn run_ui_window(config: UiConfig, receiver: Receiver<UiUpdate>, action_tx: Sender<UiAction>) {
    ui_log("run_ui_window: starting Win32 candidate window");

    // Register window class
    let class_name: Vec<u16> = "pyrust_candidate\0".encode_utf16().collect();
    // White background brush for the window class (prevents black corners)
    // SAFETY: CreateSolidBrush creates a GDI brush handle; valid until DeleteObject.
    let class_bg_brush = unsafe { CreateSolidBrush(COLORREF(0xFFFFFF)) };
    let wnd_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: HINSTANCE::default(),
        // SAFETY: LoadCursorW with system cursor IDC_ARROW is always valid.
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        hbrBackground: class_bg_brush,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: wnd_class fields are valid for the duration of this call.
    unsafe { RegisterClassExW(&wnd_class) };

    // Create the window (hidden, no-activate, tool window, topmost)
    let ex_style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST;
    let title: Vec<u16> = "pyrust\0".encode_utf16().collect();
    // SAFETY: class_name and title are valid UTF-16 null-terminated buffers that
    // outlive this call. Parent/hMenu/hInstance are None per WS_POPUP usage.
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            0, 0, 400, 60,
            None,
            None,
            None,
            None,
        )
    };

    let hwnd = match hwnd {
        Ok(h) => {
            ui_log(&format!("CreateWindowExW OK: hwnd={:?}", h.0));
            h
        }
        Err(e) => {
            ui_log(&format!("CreateWindowExW FAILED: {e}"));
            return;
        }
    };

    // Create font
    let font_size = config.font_size.max(12) as i32;
    let font_name: Vec<u16> = "Microsoft YaHei\0".encode_utf16().collect();
    // SAFETY: font_name is a valid null-terminated UTF-16 buffer. CreateFontW
    // returns a GDI handle that is valid until DeleteObject is called.
    let font = unsafe {
        CreateFontW(
            -font_size, 0, 0, 0, 400, 0, 0, 0,
            FONT_CHARSET(1),
            FONT_OUTPUT_PRECISION(0),
            FONT_CLIP_PRECISION(0),
            FONT_QUALITY(5),
            0,
            PCWSTR(font_name.as_ptr()),
        )
    };

    // Colors
    let pinyin_color = COLORREF(0x999999);
    let text_color = COLORREF(0x1A1A1A);
    let index_color = COLORREF(0xB0B0B0);
    // SAFETY: CreateSolidBrush returns GDI brush handles; cleaned up in message loop exit.
    let bg_brush = unsafe { CreateSolidBrush(COLORREF(0xFFFFFF)) };
    let hover_brush = unsafe { CreateSolidBrush(COLORREF(0xE3EDFB)) };

    // Initialize state
    let state = UiState {
        window: CandidateWindow::new(config),
        font,
        pinyin_color,
        text_color,
        index_color,
        bg_brush,
        hover_brush,
        font_size,
        action_tx,
        last_caret_x: 0,
        last_caret_y: 0,
    };
    *UI_STATE.lock().expect("UI_STATE poisoned during init") = Some(state);

    // Spawn thread to receive UiUpdate and forward to WndProc
    let hwnd_raw = hwnd.0 as isize;
    thread::spawn(move || {
        let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
        let mut count = 0u32;
        for update in receiver {
            let vis = update.visible;
            let cands = update.candidates.len();
            count += 1;
            ui_log(&format!("update #{count}: visible={vis} candidates={cands}"));
            {
                let mut guard = UI_STATE.lock().expect("UI_STATE poisoned in update thread");
                if let Some(ref mut state) = *guard {
                    state.window.apply_update(update);
                }
            }
            // SAFETY: hwnd is a valid window handle captured before thread spawn.
            let res = unsafe { PostMessageW(Some(hwnd), WM_CANDIDATE_UPDATE, WPARAM(0), LPARAM(0)) };
            if let Err(e) = res {
                ui_log(&format!("PostMessageW failed: {e}"));
            }
        }
        ui_log("UiUpdate channel closed");
    });

    // Message loop
    ui_log("entering message loop");
    let mut msg = MSG::default();
    loop {
        // SAFETY: msg is a valid MSG struct. GetMessageW/TranslateMessage/
        // DispatchMessageW are standard Win32 message loop calls.
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 == 0 || ret.0 == -1 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Cleanup
    {
        if let Some(state) = UI_STATE.lock().expect("UI_STATE poisoned in cleanup").take() {
            // SAFETY: These GDI handles were created in this function and are
            // no longer referenced after UI_STATE is taken.
            unsafe {
                let _ = DeleteObject(state.font.into());
                let _ = DeleteObject(state.bg_brush.into());
                let _ = DeleteObject(state.hover_brush.into());
            }
        }
    }
    ui_log("message loop exited");
}

// ── WndProc ─────────────────────────────────────────────────────────────

// SAFETY: This is a Win32 window procedure callback invoked by the OS.
// All parameters (hwnd, wparam, lparam) are provided by the system and valid.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut guard = UI_STATE.lock().expect("UI_STATE poisoned in WM_PAINT");
            if let Some(ref mut state) = *guard {
                paint_window(hwnd, state);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 as i32) & 0xFFFF;
            let y = ((lparam.0 as i32) >> 16) & 0xFFFF;
            let guard = UI_STATE.lock().expect("UI_STATE poisoned in WM_LBUTTONDOWN");
            if let Some(ref state) = *guard {
                if let Some(idx) = hit_test(x, y, state) {
                    let _ = state.action_tx.send(UiAction::SelectCandidate(idx));
                }
            }
            LRESULT(0)
        }
        WM_CANDIDATE_UPDATE => {
            ui_log("WM_CANDIDATE_UPDATE received");
            let mut guard = UI_STATE.lock().expect("UI_STATE poisoned in WM_CANDIDATE_UPDATE");
            if let Some(ref mut state) = *guard {
                let has = state.window.visible && !state.window.candidates.is_empty();
                ui_log(&format!("WM_CANDIDATE_UPDATE: visible={} cands={}", state.window.visible, state.window.candidates.len()));
                if has {
                    layout_and_show(hwnd, state);
                    ui_log("layout_and_show done, showing window");
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                } else {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            } else {
                ui_log("WM_CANDIDATE_UPDATE: UI_STATE is None!");
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Paint ───────────────────────────────────────────────────────────────

// SAFETY: Caller guarantees hwnd is valid and state GDI handles are live.
unsafe fn paint_window(hwnd: HWND, state: &mut UiState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let has = state.window.visible && !state.window.candidates.is_empty();
    if !has {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    // Background with rounded corners
    let mut rect = std::mem::zeroed::<windows::Win32::Foundation::RECT>();
    let _ = GetClientRect(hwnd, &mut rect);
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;

    SetBkMode(hdc, TRANSPARENT);

    let old_brush = SelectObject(hdc, state.bg_brush.into());
    let _ = Rectangle(hdc, 0, 0, w, h);

    let old_font = SelectObject(hdc, state.font.into());

    let mut y_offset = 8i32;

    // Pinyin
    if !state.window.pinyin.is_empty() {
        SetTextColor(hdc, state.pinyin_color);
        let pinyin_w: Vec<u16> = state.window.pinyin.encode_utf16().collect();
        let _ = TextOutW(hdc, 12, y_offset, &pinyin_w);
        y_offset += state.font_size + 6;
        // Separator
        let pen = CreatePen(PS_SOLID, 1, COLORREF(0xE6E6E6));
        let old_pen = SelectObject(hdc, pen.into());
        let _ = MoveToEx(hdc, 8, y_offset, None);
        let _ = LineTo(hdc, w - 8, y_offset);
        SelectObject(hdc, old_pen);
        let _ = DeleteObject(pen.into());
        y_offset += 6;
    }

    // Candidates — horizontal layout
    let candidates = state.window.page_candidates();
    let mut x_offset = 12i32;
    for (i, candidate) in candidates.iter().enumerate() {
        let idx_str = format!("{}.", i + 1);
        let idx_w: Vec<u16> = idx_str.encode_utf16().collect();

        // Index number (dimmed)
        SetTextColor(hdc, state.index_color);
        let _ = TextOutW(hdc, x_offset, y_offset, &idx_w);

        let mut idx_size = std::mem::zeroed::<windows::Win32::Foundation::SIZE>();
        let _ = GetTextExtentPoint32W(hdc, &idx_w, &mut idx_size);
        x_offset += idx_size.cx + 2;

        // Candidate text
        SetTextColor(hdc, state.text_color);
        let text_w: Vec<u16> = candidate.text.encode_utf16().collect();
        let _ = TextOutW(hdc, x_offset, y_offset, &text_w);

        let mut text_size = std::mem::zeroed::<windows::Win32::Foundation::SIZE>();
        let _ = GetTextExtentPoint32W(hdc, &text_w, &mut text_size);
        x_offset += text_size.cx + 16; // spacing between candidates
    }

    SelectObject(hdc, old_font);
    SelectObject(hdc, old_brush);
    let _ = EndPaint(hwnd, &ps);
}

// ── Layout & Position ───────────────────────────────────────────────────

// SAFETY: Caller guarantees hwnd is valid and state GDI handles are live.
unsafe fn layout_and_show(hwnd: HWND, state: &mut UiState) {
    let candidates = state.window.page_candidates();
    if candidates.is_empty() {
        return;
    }

    let hdc = GetDC(Some(hwnd));
    let old_font = SelectObject(hdc, state.font.into());

    // Calculate total width for horizontal layout
    let mut total_width = 0i32;
    for (i, c) in candidates.iter().enumerate() {
        let idx_str = format!("{}.", i + 1);
        let idx_w: Vec<u16> = idx_str.encode_utf16().collect();
        let mut idx_size = std::mem::zeroed::<windows::Win32::Foundation::SIZE>();
        let _ = GetTextExtentPoint32W(hdc, &idx_w, &mut idx_size);

        let text_w: Vec<u16> = c.text.encode_utf16().collect();
        let mut text_size = std::mem::zeroed::<windows::Win32::Foundation::SIZE>();
        let _ = GetTextExtentPoint32W(hdc, &text_w, &mut text_size);

        total_width += idx_size.cx + 2 + text_size.cx + 16;
    }

    SelectObject(hdc, old_font);
    ReleaseDC(Some(hwnd), hdc);

    let pinyin_h: i32 = if state.window.pinyin.is_empty() { 0 } else { state.font_size + 18 };
    let width = (total_width + 24).clamp(150, 900);
    let height = pinyin_h + state.font_size + 24;

    // Prefer TSF-provided position (from ITfContextView::GetTextExt) over
    // Win32 caret APIs which are unreliable in TSF IME environments.
    let (x, y) = if state.window.position.0 > 0 && state.window.position.1 > 0 {
        let (cx, cy) = state.window.position;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let mut px = cx;
        let mut py = cy + 4; // below the caret
        if px + width > screen_w { px = screen_w - width - 4; }
        if py + height > screen_h { py = cy - height - 4; }
        if px < 0 { px = 0; }
        if py < 0 { py = 0; }
        (px, py)
    } else {
        caret_position(width, height, state.last_caret_x, state.last_caret_y)
    };
    state.last_caret_x = x;
    state.last_caret_y = y;
    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_NOACTIVATE);
}

fn caret_position(width: i32, height: i32, last_x: i32, last_y: i32) -> (i32, i32) {
    // SAFETY: GetGUIThreadInfo queries the foreground thread's caret position.
    // rcCaret is in client coordinates of hwndCaret (often a child control),
    // so we must use hwndCaret for ClientToScreen — not GetForegroundWindow().
    unsafe {
        let mut pt = std::mem::zeroed::<windows::Win32::Foundation::POINT>();
        let mut got_caret = false;

        // Try GetGUIThreadInfo(0) — queries the foreground thread's caret
        let mut gui = std::mem::zeroed::<GUITHREADINFO>();
        gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(0, &mut gui).is_ok() && !gui.hwndCaret.is_invalid() {
            pt.x = gui.rcCaret.left;
            pt.y = gui.rcCaret.bottom;
            // Use hwndCaret (the window that owns the caret) for coordinate
            // conversion — not GetForegroundWindow(), which may be a different
            // top-level window and cause coordinate drift.
            let _ = windows::Win32::Graphics::Gdi::ClientToScreen(gui.hwndCaret, &mut pt);
            got_caret = true;
        }

        // Fallback: GetCaretPos (works if this thread owns the caret)
        if !got_caret && GetCaretPos(&mut pt).is_ok() {
            let fg = GetForegroundWindow();
            if !fg.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(fg, &mut pt);
                got_caret = true;
            }
        }

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        // If we couldn't get a valid caret position, reuse the last known
        // position instead of falling back to (0, 0) which pins to screen corner.
        let (base_x, base_y) = if got_caret && pt.x > 0 && pt.y > 0 {
            (pt.x, pt.y)
        } else if last_x > 0 && last_y > 0 {
            (last_x, last_y - 4) // undo the +4 from a previous call
        } else {
            (0, 0)
        };

        let mut x = base_x;
        let mut y = base_y + 4; // just below the caret

        if x + width > screen_w { x = screen_w - width - 4; }
        if y + height > screen_h { y = base_y - height - 4; }
        if x < 0 { x = 0; }
        if y < 0 { y = 0; }

        (x, y)
    }
}

// ── Hit test ────────────────────────────────────────────────────────────

/// Hit test for horizontal layout — returns the candidate index at (x, y).
/// For now, we don't have per-candidate x positions stored, so we return None
/// for clicks. Number key selection is the primary interaction method.
fn hit_test(_x: i32, _y: i32, _state: &UiState) -> Option<usize> {
    // TODO: track per-candidate rectangles for click selection
    None
}

// ── Global channel init ─────────────────────────────────────────────────

/// Initialize global UI channel and start the window thread.
/// Returns (ui_tx, action_rx):
/// - ui_tx: bridge sends UiUpdate to the UI
/// - action_rx: bridge reads UiAction from the UI
pub fn init_global_ui(config: &UiConfig) -> (Sender<UiUpdate>, Receiver<UiAction>) {
    let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<UiUpdate>();
    let (action_tx, action_rx) = crossbeam_channel::unbounded::<UiAction>();

    let cfg = config.clone();
    thread::Builder::new()
        .name("ui-win32".into())
        .spawn(move || {
            run_ui_window(cfg, ui_rx, action_tx);
        })
        .expect("failed to spawn UI thread");

    (ui_tx, action_rx)
}
