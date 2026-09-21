# LogiPeek

[English](README.md) | 简体中文

## 项目介绍

LogiPeek 是一个轻量、非官方的 Windows 原生托盘应用，带有紧凑设置窗口和 Logitech HID++ 电池/DPI 命令行诊断。它完全在本机运行。

## 动机

它面向希望以轻量方式查看鼠标电量并控制运行时 DPI、且不想安装完整设备套件的用户。它不是 Logitech G HUB 的完整替代品：没有配置文件、按键重映射、RGB、宏、后台服务、云账户或板载内存编辑器。

## 状态与目标

无参数启动时，LogiPeek 运行原生 Windows 通知区域程序并打开紧凑设置窗口；内部 `--startup` 启动只进入托盘。GUI/托盘使用进程内 validated target 并做快速重新验证，CLI 写入仍执行完整唯一目标预检。已于 2026-09-19 在 Windows 上实测一个接收器配置；详见 [实机观察](docs/hidpp.md#baseline-hardware-observation)。

LogiPeek 通过 HID++ 能力探测机制，目标是尽可能兼容不同型号的 Logitech 鼠标。自动化测试包含 34 项协议测试，以及状态、设置和滑块测试。

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
- 无参数运行时启动原生 Win32 托盘。菜单显示当前电池/DPI、Refresh、Exit，以及固定的 400/800/1600/3200 DPI 选项；设备不支持的选项保持可见但禁用，当前 DPI 匹配时会显示勾选。
- 使用一个阻塞等待的 HID worker，每 60 秒刷新电池；DPI 只在启动、托盘写入后和手动 Refresh 时读取。命名互斥量阻止重复托盘实例，但不限制 CLI 命令。
- 提供 400 x 640 逻辑像素的紧凑原生 Win32 设置窗口，支持 Per-Monitor DPI，显示电池状态、当前 DPI、能力驱动滑块、四个 preset、设备选择、开机启动、低电量提醒、内联操作状态和 Refresh。
- 同时支持离散 DPI 列表和步进范围。拖动只更新 pending preview；释放时通过同一安全 setter 最多提交一次写入，失败后回到硬件实际值。
- 支持编辑四个 preset slot、选择 System、Light 或 Dark，并在英文与简体中文之间实时切换窗口和托盘；当前设备不支持的 preset 仍可保存，但按钮会禁用。
- 首次运行或从缺少有效 `language` 字段的旧配置升级时，先显示窗口内语言选择页；选择保存后才进入正常设置页。
- 使用同步临时文件和同卷原子替换，将容错、可读设置保存在 `%LOCALAPPDATA%\LogiPeek\settings.ini`。仅“开机启动”会管理当前用户 Run 项中的 LogiPeek value；不使用服务、计划任务或数据库。
- 单个可写 DPI 设备会自动选用；多个可写设备必须由用户明确选择。持久化内容仅为派生 opaque ID，Battery/DPI 始终绑定同一 endpoint。
- 可按真实电量百分比显示中英文低电量托盘通知；默认阈值 20%，每次跌破只提醒一次，恢复 5% 后才重新允许提醒。
- 关闭设置窗口会隐藏到托盘；`Open LogiPeek` 或托盘激活会重新显示同一窗口；Tray Exit 执行干净关闭。

## 兼容性与限制

LogiPeek 可能将同一物理设备显示为多个接口，并且刻意不对其去重。`0xFF` 端点可能是直连设备或接收器端点；有响应的 `1–6` 槽位只是转发槽位候选。两者都不能确认接收器系列、配对关系或完整的已配对设备清单。

已在 Windows 上对一个 `046D:C547` USB 接收器配置验证枚举、协议 probe、动态 feature 检测、`0x1004 v3` 电池读取和一次完整 `0x2201 v2` DPI 读取。槽位 `0x01` 在线后，电池连续 5 次成功，均为 42%、Good、可充电、正在放电；尚未解释的外部电源原始指示值为 `0x00`。DPI 返回 1 个传感器、当前 1300、默认 800，以及 100–25600、步长 50 的范围。其他运行仍会在槽位 probe 阶段超时，因此端点可达性仍有间歇性。这不能识别鼠标型号，也不能证明支持所有接收器。蓝牙、直连 USB 鼠标、其他接收器系列及其他连接方式仍未验证。

已在同一配置上、`logi_lamparray_service` 运行期间完成运行时 `0x2201` DPI 写入实机验证。fresh read 返回 1300 DPI；一次 at-most-once function 3 请求将其改为相邻合法值 1350，安全即时读回与独立 `--dpi` 进程都确认了 1350。第二次 at-most-once 请求恢复 1300，并再次由即时读回和独立进程确认。两次 setter acknowledgement 均超时，因此结果由读回确认，而不是由 ACK 确认；两个命令都没有重试 function 3。该观察不能证明所有 Logitech 设备都兼容。

托盘命令路径也在该配置上完成实测。启动状态为电量 41%、DPI 1300；1600 preset 改变了真实硬件运行时值，独立 CLI 进程读回 1600。随后恢复原始 1300 并再次独立确认。Refresh、单实例和干净 Exit 也已验证。本次菜单命令由程序驱动，并非使用鼠标指针手动点击。

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

无参数执行 `cargo run` 可启动托盘与设置窗口。

四个 preset、外观选择与 GUI 语言会保存在本机。DPI 修改仍只是设备运行时值，不会写入板载配置；CLI 输出保持英文。

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

LogiPeek 没有账户、遥测、分析、上传、云端调用、运行时网络请求或后台服务。Win32 UI 只在状态或输入变化时重绘；托盘使用消息等待和一个阻塞 worker 队列，不做 busy polling。worker 每 60 秒刷新电池，并串行执行全部 HID 操作。每条已提交 DPI 命令最多进行一次物理写入。诊断输出省略序列号和完整 HID 路径。

## 路线图

- 将验证范围从已观察的 Windows USB 接收器扩展到直连和蓝牙设备。
- 仅在协议证据充分时改进设备与接收器的解释。
- 为更多电池格式和 DPI 数据加入经过验证的读取。
- 后续可继续改进原生窗口无障碍支持和多设备硬件覆盖。设备 profile、`0x2202` 写入、RGB、宏、重映射、后台服务和自动更新仍不在当前阶段范围内。LogiPeek 不轮询版本；用户从本仓库自行获取新版本。

## 贡献

请提供所用命令、脱敏后的 `--diag` 输出、Windows 版本以及准确的观察结果。请勿包含序列号、完整 HID 路径或个人资料。贡献须保持本地优先设计，不复制 GPL 源码，并将硬件证据与实现说明分开记录。

## 许可证

MIT，见 [LICENSE](LICENSE)。

## 免责声明

LogiPeek 是一个非官方开源项目，与 Logitech 无隶属、认可或赞助关系。“Logitech”和 HID++ 仅用于识别相关协议与硬件。
