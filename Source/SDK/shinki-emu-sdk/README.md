# ShinkiEMU SDK

SDK 提供移植核心共同使用的模块，当前替代 Musica 内部的文字资源管理、纹理缓存、归档公共类型和音频解码。CMVS 与 Musica 共用错误、归档、缓存、文字、纹理驻留和音频模块；两个核心均使用原生 C ABI v3；SDK 不提供另一套动态加载协议，成熟外部核心可直接依赖 `shinki-emu-abi`。

默认启用 `text`、`image`、`archive` 和 `cache` feature，`audio` 单独选择。仅需要格式错误类型的消费者使用 `default-features = false`；CMVS 选择 `archive/cache/image/audio/text/runtime/device-audio`，原生文字规则留在核心，字形排版与资源生命周期复用 SDK。`CoreError` 保留稳定 code 与说明，不能直接把未经审查的 message 写入日志；核心仍负责产生不含私有数据的错误。

`decode_audio` 复用 Symphonia 和 Kira，将完整单/双声道音频解码为浮点帧。同步调用借用 `&[u8]`，调用方保留归档或容器的字节所有者，返回的 PCM 独立持有采样；不要求将共享缓冲或容器子范围复制为 `Vec`。调用方指定最大帧数并持有取消标志；探测前和包之间检查取消，采样率变化、非有限采样、解码失败或超预算均明确失败。Musica 的旧解码模块已删除，读取资产时保留 `roukaku-byte-source::OwnedByteBuffer`；CMVS 用同一函数处理普通音频和经核心校验的 MGV 内嵌 Ogg。旧的按值 `Vec` 参数直接改为借用切片，不保留并行解码入口。此模块不创建音频设备或 worker，也不代替流式长媒体播放。

`archive` 复用 `roukaku-byte-source` 提供本地归档节点、读取结果和索引校验，不提供 Host VFS、registry 或挂载工厂。`ArchiveManifest` 使用 `shinki.emu.archive_manifest.v2`，是核心内部索引，不是产品 package。来源与条目的 `source_revision` 表示核心提供的索引/文件版本身份，并不声称是完整资源内容校验和；条目必须与其来源版本一致。核心可使用已读取的归档索引、分卷长度和修改时间生成该身份，不要求启动时扫描全部资产。`CacheIdentity` 使用相同字段及 v2 身份命名空间；旧字段和旧 manifest schema 直接移除，不保留兼容接口。Musica 原来的 `Paz*` 公共读取类型已改为相同的 `Archive*` 类型，不保留别名。

`read_game_profile` 有界读取游戏目录内私有 JSON。`read_configured_profile_bytes` 供明确支持私有外置配置的核心读取有界原始字节：绝对路径可以位于游戏目录外，相对路径仍须位于游戏目录内；返回值同时保留原始字节 SHA-256 和 canonical 游戏根。`resolve_game_file` 始终校验资源相对路径和 canonical 目录归属，不因 profile 外置而放宽资源路径。错误码使用公共 `SHINKI_EMU_PROFILE_*`，区分缺失、权限、非文件、格式、路径和大小限制，给出配置操作，不回显输入或私有路径、不写文件。`cache` 复用原 Musica 的有界明文缓存、来源身份校验、淘汰和系统权限实现；是否创建磁盘缓存由核心明确选择。

`TextScene` 使用 `roukaku-text` 的字体排版和批量字形资源管理；调用方传入字体 provider、完整可见文字区域和颜色/位置，获得生命周期在前、绘制在后的 `SceneCommand`。区域消失时释放字形，共享字形在同一帧统一处理。SDK 不规定消息框坐标、字体或 VM 状态。返回命令须按顺序提交给同一渲染器；渲染失败关闭会话，不丢弃资源命令后继续。

`TextScene::layout_shared` 返回同一 provider/cache 的只读 `TextLayoutResult`，供原生核心读取 `ShapedGlyph::advance`。它不上传字形，也不创建可见区域；`frame` 复用该入口并负责资源生命周期。字体缺失、覆盖不足和其他 Error/Blocking 诊断均原样返回，不以 bitmap 宽度或字号比例补出度量。现有调用不需要迁移；CMVS 比例文字布局接入此接口。

`TextureCache` 复用 `roukaku-media-core::TextureFrame`、`image` 和 `lru`，同时限制条目数、尺寸和缓存持有的 RGBA 字节数。标准图片解码在转换 RGBA 前检查尺寸与分配预算；专用格式由核心解码后调用 `insert`。淘汰不会使外部持有的帧失效，预算不代表 GPU 或所有调用方的总内存。

`SceneTextureResidency`（`image`）把每帧新建的 `Texture` 绘制命令改为既有公共 `UploadTexture/Sprite/ReleaseResource`；Musica 的所有场景/影片提交及 CMVS 的 texture/text/clear 提交使用同一模块。不可变像素 owner 的地址、长度和尺寸构成身份，保留原帧引用防止地址复用误命中，不逐帧比较或 hash 正文；相同资源的多个绘制共享上传，移动、透明度和裁剪不重传像素。不再出现在当前场景的资源在下一次提交释放，当前工作集仍受渲染器资源/字节预算限制，atlas 可保留有界容量。模块不拥有 GPU 或第二套 renderer，不改变逻辑尺寸、混合空间、ANI 原点、VM 或存档。

一个 residency 实例对应一个 renderer，在提交前调用 `prepare`；输入为本帧尚未转换的命令，不能再次转换上次的输出。上传/释放位于绘制前；没有资源变化时不额外分配命令向量。和 `TextScene` 一样，渲染失败关闭该 renderer/cache 对应会话，不能丢掉资源命令继续；重建场景/设备时创建新的实例。

`StageCanvas` 只包装公共 `Canvas2D` 的逻辑舞台到 raster 输出映射；`TextureAsset` 将物理 `TextureFrame` 与显式 logical extent、logical origin 分开。Family 可以用原资源的逻辑几何承载不同密度的替换像素，SDK 不定义具体归档、命名或替换策略，也不创建第二套纹理缓存。

`subtitle` 是可选的共享 ASS 消费路径：`subtitle` 模块导出 `roukaku-subtitle` 的借用帧、颜色和字体接口，`AssScene` 在调用线程持有原生 renderer。调用方提供实际媒体时间（微秒）、视频 storage 尺寸、实际 viewport 和原生转换的 SDR `VideoColorSpace`。`update` 只产生资源变更：libass 原生 0 不分配/上传，1 只更新位置/颜色，2 才复制有界 Alpha8 mask；mask 使用内容 revision，避免同一帧释放并重传相同资源 ID。`append_draws` 保留原生 run 顺序。轨道替换、seek generation 更换或结束前调用 `release_resources`；新轨道使用该 renderer 内独立 namespace，随后在同一线程销毁。没有 unsafe Send、系统字体替代、全视频帧复制或逐帧 Lua。

`video-ffmpeg` 的 `VideoFramePacket.color` 描述实际 BGRA8 呈现转换。AVFrame 的已知 matrix/range 配置 swscale，随后从 `sws_getColorspaceDetails` 读取实际使用的系数和范围；无标签 WMV/AVI 沿用现有解码器的 SDR 呈现策略，不按 raster 尺寸猜矩阵，也不把输出 BGRA 误报为输入 RGB。显式 HDR/未支持标签仍保留拒绝状态，这不宣称恢复了未标记源的原始色彩意图。storage 与 viewport 分开传递。公开 `subtitle_scene` fixture 覆盖驻留、position-only、暂停、回退、销毁重建和带偏移 viewport。


SDK 不创建 EngineSession、VN session、PlatformHost 或 package，不拥有游戏存档与归档语义。Musica 直接使用 RoukakuEngine 的 `WgpuOffscreenRenderer`，SDK 无需再包一层渲染 provider。

```sh
cargo test -p shinki-emu-sdk
cargo test -p shinki-emu-musica gpu_text_updates -- --ignored
```

第二项需要硬件 GPU；不能用软件 adapter 的结果作为通过。


## 原生 owner 辅助

`runtime` 提供原生 owner 内部使用的 Rust 配置、输入、窗口命令、错误和不可变帧值，使用标准库所有权；这些值不跨动态库边界，不包含模块 vtable、宿主 advance 或旧协议 reader。实际 C ABI 配置始终先通过 `shinki-emu-abi::validate_config`，再转换成核心拥有的值。

`native` 共享 session/context epoch、原生服务失效、取消与窗口结果检查。`native_input` 将有界 C 输入复制成有序的拥有型事件，并按 `StageCanvas` 逆映射指针；不调用 VM。`lease_cpu` 只让独立 owner 持有不可变像素与有界租约计数，关闭会话不影响已有 CPU 帧的读取或归还。

`device-audio` 提供 `NativeAudioOutput` 和 `NativeAudioConverter`：在核心音频线程创建 CPAL 设备/stream，将来源 PCM 映射到设备通道并按实际采样率重采样，复用转换与队列存储。写入背压观察核心停止标志，设备错误唤醒队列并上报，不切换到静音后端。混音规则、播放位置、影片时钟和 worker 的创建/关闭仍归核心所有。

`NativeAudioOutput::progress()` 返回 `NativeAudioProgress`，单位是 48 kHz 来源帧。`accepted_frames` 累计每次返回 `Ok(Some(_))` 的 `write`。`played_frames` 取设备队列已消费的样本，减去 CPAL 回调报告的输出延迟和重采样器延迟，再换算回 48 kHz；欠载补出的静音不计入。该值单调不减，且不超过 `accepted_frames`。核心用它推进声道位置和影片时钟；设备故障仍通过 `check()` 得到。
