# CMVS

CMVS 通过 C ABI v3 导出 `shinki_emu_query`，协议为 `shinki.emu.native.c.v3`。核心自行打开 CPZ5、执行 PS2A、混音、解码、渲染并维护原生存档。Manager 只提交输入和控制、读取状态与不可变 CPU 帧，不推进 VM。

## 会话与媒体

模块 owner 跨会话保留原生 `wgpu::Instance`；每次会话的 VM、Scene 和媒体对象都在 CMVS owner 线程创建和销毁。输入进入有界队列，暂停与恢复清空时钟积累，逻辑调度保持 60 Hz；指令和单次 tick 有预算。场景使用 SDK `StageCanvas`、`TextureCache` 与共享 `WgpuOffscreenRenderer`，逻辑舞台和物理 raster 分开。GPU 初始化失败明确报错。

C ABI 的帧与状态查询只读取已发布快照。CPU 帧声明 RGBA8/sRGB、top-down、ignored alpha，保留原字节和 stride；租约只持有不可变像素与独立计数器，关闭会话后仍能在其他线程读取和归还。配置校验、输入转换、窗口请求的迟到结果拒绝、取消上下文及 CPU 租约复用 SDK 的 `native` 模块。

音频 owner 将原有交替 BGM、crossfade、淡出及通道音效混到 48 kHz stereo F32，再由 SDK `NativeAudioOutput` / `NativeAudioConverter` 转换到真实设备格式并写入有界设备队列。缺失或失败的设备会使打开或播放失败，不建立静音替代。MGV 的 slot-2 音频时钟、帧计数和已有 Theora 解码路径保持原生规则；`video-ffmpeg` 启用实际视频解码。

取消会失效原生窗口服务、唤醒音频队列，并让音频解码与 VM 指令循环观察同一停止标志。关闭等待 VM owner、音频 owner 和视频 decoder 结束；一个清理错误不会跳过其他 owner 的清理。进程同一时间只允许一个 CMVS 会话，失败打开也释放会话租约。

## 资源和原生状态

`profile_file` 默认读取 `cmvs.profile.json`，schema 为 `shinki.emu.cmvs.profile.v2`；`entry_script` 默认 `start.ps3`。配置的 `archives` 是有序 `{ role, path }` 列表，`loose_files` 按唯一 role 映射相对文件。重复角色、重复来源、错误 schema、未支持的 CPZ 版本和越界路径明确拒绝。探测只检查游戏根及 `data/pack` 下的 CPZ5 标头，不依赖作品名称。

启动先从 `cmvs.cfg` 的 `CMVS_SYSTEM_MAIN.SCRIPT_INIT_PATH` 建立 Script slot 0，再加载 bootstrap。脚本、纹理和八个原生搜索域保持独立；按槽号和归档内原始索引顺序查找，不扫描其他域，不用同名 CPZ 资源替代已选中的损坏 loose 文件。文件读取与解码成功后才安装脚本 frame 的入口、身份、名称索引、字符串长度和初始数据段；frame 限 0–3。

挂载只读取 header/index 和来源大小、修改时间；`manifest()` 或持久明文缓存需要内容身份时才计算完整 hash。资源读取仍检查源变化。共享 `OwnedByteBuffer` 范围和流保留原分配；缓存释放不使现有读者失效。输入脚本和单个音频资源上限为 64 MiB，完整 PCM 解码另受帧预算限制。

PS2A scheduler、返回码弹栈和原生 cmd693/694 存读档沿用核心已有实现。菜单打开时保存悬挂的 story context；`StorySnapshot` 保留 `slot_objects`、`texture_parents`、`texture_children` 和 `retained_slot_placements`，恢复时不拿菜单的临时调度状态替代剧情。系统存档与 slot codec 校验版本、长度、身份、校验和和压缩边界；损坏候选不能部分替换活动状态或覆盖原文件。

全局字符串是 128 个独立 Shift-JIS 值；命令立即复制已解析名称，后续切 frame、替换全局槽或根脚本不会改变既有值。tag `0x00` 的只读池、tag `0x40` 的可写 data segment 与全局来源保持区分；`0x201/0x170` 赋值按源记录的 tag/value 读取，再按目标记录写入。未恢复字符串域明确拒绝。原生槽容量含 NUL 为 1024 字节，超长、内嵌 NUL 和非法编码不截断写入。

命令先统一读取并弹出原生参数，再交给内存、资源、对象、滤镜、演出、纹理、脚本、设置或消息处理器。执行失败后的部分 VM 状态不能继续 dispatch，也不能仅清除 `dispatch_stopped` 后重试；只能结束会话或恢复独立校验过的有效状态。内部快照不兼容旧版本。

## 输入、窗口与文字

case 416 读取确认键的释放锁存和按住状态，重复轮询不消费；417 只清除按下、释放锁存。Enter、Space、数字键盘 Enter 和主指针按钮各自跟踪；同 tick 按下与释放不丢失，失焦或挂起清空输入。751 读取 skip/auto 模式和 interpreter override，不把同步资源加载当作自动跳读。

352 保存窗口标题前缀，353 复制后缀并组合标题；354 独立维护原生保存标题缓冲。`FONT_SHIFT_JIS` 决定 CP932 全角空格或两个 ASCII 空格。标题组合达到原生 128 字节缓冲边界时拒绝并保留旧后缀。窗口标题和 ICO 图标按类型排队，互不覆盖；图标缺失遵守原生 best-effort，越界路径和资源超预算仍报错。窗口操作通过 C ABI window service 执行。

原生文字对象保留 12 槽、字体 owner、字号/间距、双颜色、禁则、光标和有符号保留值规则。静态文字、控制符、换行与静态 ruby 经同一 SDK `TextScene` 批量提交；固定/比例前进量来自实际 glyph 度量，描边与渐变沿用已有实现。

字体从本机数据库精确匹配，不替换成 generic 或 Noto。配置按原生 `CONFIG_PUT/REG` 选择 INI 或 Windows HKCU 的 32-bit 视图；缺配置保留默认“ＭＳ ゴシック”。INI 有界读取 CP932 或带 BOM 的 UTF-16LE，不写 registry 或原配置。Documents/AppData 保存根与未恢复命令继续明确拒绝。

## 构建与回归

```sh
cargo check -p shinki-emu-cmvs --all-targets --locked
cargo test -p shinki-emu-cmvs --locked
cargo build -p shinki-emu-cmvs --locked
```

`build.rs` 记录 `core-build.json` 与实际许可证输入，库路径由 Cargo 的 compiler-artifact 输出提供。回归保留格式、调度、参数弹栈、场景和原生存档边界；owner 用公开合成 PS2A 程序检查排队输入、快照读取、取消/关闭与跨关闭 CPU 租约，音频用真实有界队列检查背压取消。创建 Scene 的回归需要硬件 GPU。

原始游戏文件、私有密钥、解码结果和截图不进入仓库。构建与合成回归不产生作品兼容性结论；真实游戏验收单独记录。
