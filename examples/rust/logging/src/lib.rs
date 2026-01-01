#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[link(wasm_import_module = "env")]
extern "C" {
    fn log(level: i32, ptr: *const u8, len: i32);
}

#[no_mangle]
pub extern "C" fn _start() {
    let message = "Hello from Rust WASM!";
    unsafe {
        log(1, message.as_ptr(), message.len() as i32); // Info level = 1
    }
}
