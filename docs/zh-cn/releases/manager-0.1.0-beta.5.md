# ShinkiEMU Manager 0.1.0-beta.5

本次预发布在现有 Manager 平台之外新增实验性 OHOS arm64 HAP，集成 Slint 管理界面及 OpenHarmony 原生窗口、输入、音频和 WebView 宿主。

HAP 使用 OpenHarmony SDK 测试 PKI 签名，并非生产证书。本地 DevEco 打包核验签名与原生库依赖闭包；托管发布交接门禁核对实际 HAP 及对应源码的 SHA256、包内容、原生依赖闭包和源码提交。设备/模拟器启动验收已获豁免，零售设备可安装性未验证。

托管平台二进制继续通过原有发布门禁。Android 通用 APK 使用发布签名密钥。每个已发布平台都提供对应源码和依赖许可声明。beta4 发布及其附件保持不变。
