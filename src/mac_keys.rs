//! macOS keyboard simulation through CoreGraphics. Unlike enigo (which asks the
//! text-input-source API for the layout and traps off the main thread), posting raw
//! key codes is safe from any thread.

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const std::ffi::c_void, keycode: u16, down: bool) -> *mut std::ffi::c_void;
    fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    fn CFRelease(cf: *const std::ffi::c_void);
}

const CMD_FLAG: u64 = 0x0010_0000;
pub const KEY_C: u16 = 8;
pub const KEY_V: u16 = 9;

/// Press and release Cmd+<key> in the focused app.
pub fn cmd_key(keycode: u16) -> Result<(), String> {
    unsafe {
        for down in [true, false] {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), keycode, down);
            if ev.is_null() {
                return Err("could not create key event".into());
            }
            CGEventSetFlags(ev, CMD_FLAG);
            CGEventPost(0, ev); // kCGHIDEventTap
            CFRelease(ev);
        }
    }
    Ok(())
}
