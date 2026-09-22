# LogiPeek v0.1.0 — Pre-release

LogiPeek is an unofficial native Windows x64 utility for Logitech HID++ devices. This pre-release provides battery monitoring, DPI reading and runtime control, fast DPI switching, a tray icon, a compact Liquid Glass style settings window, English and Simplified Chinese, low-battery alerts, Start with Windows, and selection among multiple detected devices.

Download the per-user Windows installer (`LogiPeek-0.1.0-x64-Setup.exe`) or the portable ZIP (`LogiPeek-0.1.0-x64-portable.zip`). Verify downloads with `SHA256SUMS.txt`. The installer does not require administrator privileges. The portable build runs directly after extraction.

This release is currently unsigned, so Windows SmartScreen may display a warning. The SHA256 file checks download integrity; it is not a code signature.

## Known limitations

- Broad Logitech hardware compatibility has not yet been verified.
- Forwarded HID++ endpoint availability may depend on hardware and connection state.
- Bluetooth, Bolt, and other models or connection methods have not been comprehensively tested.
- DPI changes affect the current runtime value, not onboard profiles.
- There is no automatic updater. Get later releases from https://github.com/1TcC/LogiPeek/releases.

## 简体中文

LogiPeek v0.1.0 是 Windows x64 预发布版，提供电量监测、DPI 读取与运行时控制、快速 DPI 切换、托盘、Liquid Glass 风格设置窗口、中英文界面、低电量提醒、开机启动和多设备选择。可下载当前用户安装包或免安装 ZIP，并用 `SHA256SUMS.txt` 校验完整性。

本版本目前没有代码签名，因此 Windows SmartScreen 可能显示警告。广泛的 Logitech 硬件兼容性尚未验证；转发的 HID++ 端点是否可用可能受设备和连接状态影响。蓝牙、Bolt 及其他型号尚未全面测试。没有自动更新功能；新版请到 https://github.com/1TcC/LogiPeek/releases 获取。
