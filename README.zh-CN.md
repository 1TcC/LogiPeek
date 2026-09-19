# LogiPeek

[English](README.md) | 简体中文

## 项目介绍

LogiPeek 是一个小型、非官方的 Windows 命令行工具，用于查看 Logitech HID++ 接口，并报告其公开的电池和 DPI 能力。它完全在本地作为一次性诊断工具运行。

## 动机

它面向希望以轻量方式查看鼠标电量和 DPI 能力、且不想安装完整设备套件的用户。它不是 Logitech G HUB 的完整替代品：没有配置文件、按键重映射、RGB、宏、托盘应用、后台服务或持久化配置。唯一的配置命令是用户显式执行的一次性运行时 DPI 修改。

## 状态与目标

项目仍处于早期、以读取为主的基础阶段，并包含一个严格限定的写操作：`--set-dpi <DPI>` 在完成新的安全预检后，通过 `0x2201` function 3 修改当前运行时 DPI。它枚举 Logitech HID 接口，识别部分 HID++ 响应，并且只显示设备实际返回的信息。目标是以精简的 Rust 实现提供明确的失败信息、可安全分享的诊断输出和无后台活动的工具。已于 2026-09-19 在 Windows 上实测一个接收器配置；详见 [实机观察](docs/hidpp.md#baseline-hardware-observation)。它不承诺支持具体鼠标型号或广泛的接收器兼容性。该配置已成功完成真实 `0x1004` 电池和 `0x2201` DPI 读取，但转发槽位可达性仍有间歇性。

LogiPeek 通过 HID++ 能力探测机制，目标是尽可能兼容不同型号的 Logitech 鼠标。34 项协议测试、`cargo fmt --check`、`cargo check`、`cargo clippy --all-targets --all-features -- -D warnings` 和 release build 均于 2026-09-19 通过。

## 当前功能

- 枚举 Logitech USB HID 接口，显示安全处理后的名称、ID、接口元数据，以及 native backend 可提供时重建的 report descriptor 长度。
- 仅把 usage page 为 `0xFF00` 且 usage 为 `1` 或 `2` 的接口视为 HID++ 查询候选。
- 探测端点 `0xFF` 及槽位 `1` 到 `6`；这些槽位只是有界候选，并非接收器或已配对设备清单。
- 识别 HID++ 2.x feature 协议，以及仅作识别的 HID++ 1.x；尚未实现 HID++ 1.x 寄存器功能。
- 检测电池 feature ID `0x1000`、`0x1001`、`0x1004`。有 `0x1004` 时读取 Unified Battery，并保留 `0x1000` version 0 作为回退。
- 在所选 feature 提供时，以只读方式报告电池百分比、粗略等级、充电状态、feature 版本和外部电源原始指示值；未知协议值会明确保留为未知。
- 检测 DPI feature ID `0x2201`、`0x2202`。从 `0x2201` 读取传感器数量、当前 DPI、可选默认 DPI 及支持的数值或范围。
- 提供 `--set-dpi <DPI>`，用于经过验证的、仅影响运行时的 `0x2201` function 3 修改。新的预检必须找到恰好一个 `0x2201` 端点和恰好一个传感器。请求值必须精确出现在离散列表中，或与范围步长严格对齐；无效值会被拒绝并给出最近的受支持建议，距离相等时选择较高值。
- 每个物理请求使用 750 ms 响应窗口。只有明确的只读请求最多尝试两次，且仅在 Timeout 或对应协议的 HID++ Busy 错误后重试；通用 exchange 从不自动重试。
- 提供一次性运行的 `--devices`、`--diag`、`--battery`、`--dpi` 和 `--set-dpi <DPI>` 命令。

## 兼容性与限制

LogiPeek 可能将同一物理设备显示为多个接口，并且刻意不对其去重。`0xFF` 端点可能是直连设备或接收器端点；有响应的 `1–6` 槽位只是转发槽位候选。两者都不能确认接收器系列、配对关系或完整的已配对设备清单。

已在 Windows 上对一个 `046D:C547` USB 接收器配置验证枚举、协议 probe、动态 feature 检测、`0x1004 v3` 电池读取和一次完整 `0x2201 v2` DPI 读取。槽位 `0x01` 在线后，电池连续 5 次成功，均为 42%、Good、可充电、正在放电；尚未解释的外部电源原始指示值为 `0x00`。DPI 返回 1 个传感器、当前 1300、默认 800，以及 100–25600、步长 50 的范围。其他运行仍会在槽位 probe 阶段超时，因此端点可达性仍有间歇性。这不能识别鼠标型号，也不能证明支持所有接收器。蓝牙、直连 USB 鼠标、其他接收器系列及其他连接方式仍未验证。

已在同一配置上、`logi_lamparray_service` 运行期间完成运行时 `0x2201` DPI 写入实机验证。fresh read 返回 1300 DPI；一次 at-most-once function 3 请求将其改为相邻合法值 1350，安全即时读回与独立 `--dpi` 进程都确认了 1350。第二次 at-most-once 请求恢复 1300，并再次由即时读回和独立进程确认。两次 setter acknowledgement 均超时，因此结果由读回确认，而不是由 ACK 确认；两个命令都没有重试 function 3。该观察不能证明所有 Logitech 设备都兼容。

HID++ 传输可能被其他软件同时使用。回复可能陈旧，或来自其他应用；轮换 software ID 可降低冲突，但并不独占。若诊断结果不一致，请关闭 Logitech 软件后重试。

## 构建

安装 Rust stable MSVC toolchain、Windows SDK 和 Build Tools，然后在 Developer PowerShell 或 MSVC linker 可用的环境中执行：

```powershell
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

开发时可运行：

```powershell
cargo run -- --devices
cargo run -- --diag
cargo run -- --battery
cargo run -- --dpi
cargo run -- --set-dpi 1600
```

项目通过 `hidapi` 的 `windows-native` backend 访问 HID；运行时不需要网络。

## 命令行

```text
logipeek --devices   # 候选接口与已发现能力
logipeek --diag      # 可安全分享的接口/协议详情
logipeek --battery   # 读取受支持的 0x1004 电池数据，并回退到 0x1000 v0
logipeek --dpi       # 读取受支持的 0x2201 DPI 数据
logipeek --set-dpi 1600  # 验证并修改当前运行时 DPI
logipeek --help
```

`--diag` 不显示 HID 路径和序列号。超时、接口繁忙、回复格式错误或 feature 不受支持在部分硬件上都可能发生，工具会明确显示。

`--set-dpi` 不会取整或截断。它通过通用 exchange 路径仅发送一次 function 3；遇到 Timeout、Busy、I/O 失败、回复格式错误或任何其他结果都不会重试写入。Version 1 及以上版本的确认响应必须回显传感器索引和大端序 DPI 值；version 0 不回显这些参数。收到有效确认或 setter Timeout 后，会执行安全的 function 2 读回。匹配的读回值可验证观察到的运行时结果；已确认请求若读回失败或数值不同，结果仍未验证；setter Timeout 后，匹配的读回可确认当前观察值，不同或不可用的读回则保留为不确定。非 Timeout 协议错误、I/O 错误、格式错误响应和错误回显会直接返回，不执行读回。LogiPeek 不会为了消除不确定性而再次写入。

## 隐私与性能

LogiPeek 没有账户、遥测、分析、上传、云端调用、后台服务或轮询循环。请求串行且有上限。请求发出后，回复截止时间为 750 ms，最多处理 128 个入站报文。明确的只读操作可在 Timeout 或 HID++ Busy 后重试一次；格式错误、I/O 错误和其他协议错误会立即返回。DPI setter 始终只进行一次物理写入尝试。Windows backend 的单次写入本身最多可能耗时约一秒，因此 750 ms 不是整个 exchange 的上限。诊断输出省略序列号和完整 HID 路径。

## 路线图

- 将验证范围从已观察的 Windows USB 接收器扩展到直连和蓝牙设备。
- 仅在协议证据充分时改进设备与接收器的解释。
- 为更多电池格式和 DPI 数据加入经过验证的读取。
- 当前阶段仍不包含 DPI 预设、持久化、配置文件、`0x2202` 写入、RGB、宏、重映射、GUI/托盘、后台服务、开机启动或更新。已授权的 `0x2201` 运行时 DPI 命令不会扩大这一范围。

## 贡献

请提供所用命令、脱敏后的 `--diag` 输出、Windows 版本以及准确的观察结果。请勿包含序列号、完整 HID 路径或个人资料。贡献须保持本地优先设计，不复制 GPL 源码，并将硬件证据与实现说明分开记录。

## 许可证

MIT，见 [LICENSE](LICENSE)。

## 免责声明

LogiPeek 是一个非官方开源项目，与 Logitech 无隶属、认可或赞助关系。“Logitech”和 HID++ 仅用于识别相关协议与硬件。
