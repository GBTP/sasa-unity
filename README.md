# sasa-unity

[sasa](https://github.com/Mivik/sasa) 音频引擎的 C ABI 封装，供 Unity 通过 P/Invoke 调用。

- 线程约定：所有导出函数只能在 Unity 主线程调用（引擎实例在 `thread_local` 中）。
- crate 类型：`staticlib` / `cdylib`。

## 构建

```bash
./build.sh ios       # 仅 iOS（静态库）
./build.sh android   # 仅 Android（arm64 + armv7）
./build.sh macos     # 仅 macOS（arm64 + x64）
./build.sh all       # 全部
```

构建产物会部署到 `../Anoawa/Assets/Plugins/Sasa`（Unity 工程）。

## 依赖

`sasa` 以固定 rev 从 GitHub 拉取，并用本地 fork `../sasa` 覆盖（关闭上游 oboe 的 `shared-stdcxx` feature）。因此**克隆后需要把 `sasa` 目录放到仓库同级的 `../sasa` 才能编译**。

Android 构建需要 [cargo-ndk](https://github.com/bbqsrc/cargo-ndk) 和 Android NDK。
