---
title: 核心 ABI 与 SDK
description: C ABI v3 生命周期、owner 线程、CPU 帧租约和核心包。
---

# 核心 ABI 与 SDK

## 协议与入口

原生协议为 `shinki.emu.native.c.v3`，ABI 数值版本为 **3**。动态核心导出 `shinki_emu_query`，静态集成注册相同形状的 Query 函数。Rust 契约位于 `Source/FamilyApi/shinki-emu-abi`，生成的 C 头文件为 `include/shinki_emu.h`。C++ 使用这份头文件，不另行维护 ABI 定义。

边界只传 C 布局值，包括版本头、有界借用字节、不透明句柄、状态码和函数指针；Rust 容器及异常展开不跨 ABI。动态库及不可变 descriptor 存储保持驻留。query 接收版本和函数表输出指针；不支持的版本将输出置空。query 不执行 I/O、不启动 worker，也不安装 logger。

## 生命周期

1. **query** 取得不可变 `CoreApi` 表，检查版本、大小和必需函数指针。
2. **initialize** 在普通核心操作之前接入诊断。
3. **describe** 返回 family/plugin 标识、版本、格式、类型化配置和能力。CPU surface 是必需能力；非零渲染尺寸须由 descriptor 声明并落在范围内。
4. **probe** 检查游戏目录，`NO_MATCH` 属于正常结果。返回字符串借用模块存储，到下一次普通调用前有效，宿主先复制所需元数据。
5. **open** 校验配置并创建原生会话。失败时清理资源并返回空句柄；成功返回 session/context/output 身份和 surface 元数据。
6. **input** 将输入复制为有序拥有型事件并入队；**control** 请求暂停、恢复或已保留且可安全重试的操作。指针坐标对应当前显示帧的光栅像素及身份。
7. **state**、**frame** 读取已发布状态和画面，不推进 VM。失败会话携带类型化错误；只有核心声明安全时，宿主才提供重试。
8. **cancel** 可以并发调用，必须唤醒阻塞中的原生服务，不能排在被取消操作后面等待。
9. **close** 无论返回何种结果都消耗句柄；返回前等待 worker 和回调结束。宿主先阻止新 cancel，再等已有 cancel 返回，最后 close。
10. **extension** 协商具名、带版本的 C 表。不支持时返回 `UNSUPPORTED` 和空输出。可选 `shinki.module_owner` 为真实跨会话资源提供 owner，由模块 owner 线程创建，在所有会话结束后销毁。

普通 ABI 调用在宿主 owner 线程串行执行。核心自己的原生 owner 负责 VM 推进、渲染/音频调度及资源生命周期。Manager 只提供输入、控制和宿主服务，没有 advance 回调。取消和 epoch 校验会拒绝旧会话或旧输出代次的迟到结果。

## CPU 帧租约

Ready 状态的 `FrameSurface` 包含实际尺寸、stride、像素/颜色/行序元数据、帧身份及一个不可变 CPU 租约。接管前先校验元数据和 epoch。Pending、Unchanged 不携带像素所有权；显示时遵守声明的光栅/逻辑几何和颜色解释。

每个租约只归还一次，而且要等真实上传 API 不再读取像素。owner 和 release 回调在会话关闭后仍有效，也能在其他线程调用；归还不得访问 VM、设备、窗口、session 或宿主服务。持有帧不会延长游戏会话，也不能让核心覆盖仍在借出的像素。ABI 的 `CpuFrame` 包装器即使校验失败，也会归还已接管的租约。

## SDK 职责

`Source/SDK/shinki-emu-sdk` 提供共用机制，不另建游戏运行时。`runtime` 值只在进程内部使用；`native` 复用配置校验、epoch、取消、窗口迟到结果拒绝和独立 CPU 租约；`native_input` 有界复制输入，不调用 VM。`NativeAudioOutput`/`NativeAudioConverter` 服务于核心拥有的设备线程。混音、媒体时钟、原生存档和游戏规则仍由核心负责。具体 API 见 [SDK](https://github.com/Project-Shinkirou/ShinkiEMU/tree/main/Source/SDK/shinki-emu-sdk)。

## 核心包

包内核心目录与库文件 stem 同名：

```text
<library-stem>/
  core.json
  shinki_emu_<core>.dll       # 或 libshinki_emu_<core>.so/.dylib
  build.json
  Resources/                 # 原生核心需要时提供
  licenses/
  <原生依赖文件>
```

`core.json` 使用 `shinki.emu.core_package.v1`，记录 `plugin_id`、`library`、`version`、`abi` 及文件路径/SHA256。清单覆盖除自身外的所有包文件。核心库使用新文件名前缀和 query 符号。打包检查架构、依赖闭包及许可，拒绝路径穿越、链接和大小写冲突，只发布到不存在的新目录。Manager 在安装前校验整个包，不接受旧产品协议或加载别名。命令见[构建与发布](build-and-release.md)。
