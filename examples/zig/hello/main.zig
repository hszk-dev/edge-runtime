// WASIを使わず、env::logホスト関数のみを使用

extern "env" fn log(level: i32, ptr: [*]const u8, len: i32) void;

export fn _start() void {
    const message = "Hello from Zig!";
    log(1, message.ptr, @intCast(message.len)); // Info level = 1
}
