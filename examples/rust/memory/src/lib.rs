#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// 64KB の静的バッファ
static mut BUFFER: [u8; 65536] = [0u8; 65536];

#[no_mangle]
pub extern "C" fn _start() {
    unsafe {
        // バッファを埋める（メモリ操作テスト）
        for i in 0..BUFFER.len() {
            BUFFER[i] = (i % 256) as u8;
        }
        // 最適化で消されないように
        core::hint::black_box(&BUFFER);
    }
}
