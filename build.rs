fn main() {
    // Android 上 oboe 的 C++ 部分静态链 libc++（见 ../sasa/Cargo.toml）。cc crate 只会
    // 发出 -lc++_static，而 libc++ 把 std::runtime_error / std::bad_alloc / __cxxabiv1::*
    // 这些异常和 RTTI 相关的符号留在 libc++abi 里 —— 平时由 clang 驱动自动补上 -lc++abi，
    // 但这里链接驱动是 rustc，不会补。少了它，.so 里会留下一批 UND 符号，dlopen 时才炸。
    // 顺序不能反：c++_static 由 oboe-sys 先发出，这里追加的 c++abi 排在它后面。
    // 不加 static= 前缀：那会让 rustc 自己去搜库，而它不认识 NDK sysroot；
    // 普通形式直接透传给链接器（clang，带 --sysroot），由它解析。sysroot 里
    // libc++abi 只有 .a，所以实际仍是静态链接。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=c++abi");
    }
}
