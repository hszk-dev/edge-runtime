#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

fn fib(n: u32) -> u64 {
    if n <= 1 {
        n as u64
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

#[no_mangle]
pub extern "C" fn _start() {
    let n = 25; // fib(25) = 75025
    let result = fib(n);
    // 結果を使って最適化で消されないようにする
    core::hint::black_box(result);
}
