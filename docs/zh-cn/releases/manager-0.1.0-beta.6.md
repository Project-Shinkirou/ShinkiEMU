# ShinkiEMU Manager 0.1.0-beta.6

Musica 的手动存档、快速存档和自动存档改用 eden/Perseus 原生 `.sav` 槽位及配套 `.png` 缩略图。旧测试版 `.asav` 文件不兼容，仍留在原目录，不读取、转换、移动或删除。

Musica 除 Android 和实验性 OHOS 外，也静态集成到 iOS 真机目标和两种模拟器目标的 Manager。iOS 不支持单独安装动态核心。集成版本必须通过自身精确提交的构建、打包、签名、原生依赖闭包及对应源码门禁，不能用独立移植的结果替代。

OHOS HAP 仍为实验性 OpenHarmony SDK 测试 PKI 签名包。设备、模拟器、游戏及账户启动验收已获豁免，零售设备安装性尚未验证。每个已发布平台包均附对应源码和依赖许可。已发布 Manager beta4、beta5 的资产保持不变。
