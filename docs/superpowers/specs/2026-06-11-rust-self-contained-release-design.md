<!--
文件意图：记录 Hermes Agent 发布包自包含化与 Rust 边界层改造的可行性设计，供后续实施计划评审使用。
-->

# Hermes Agent Rust 边界层与自包含发布包可行性设计

日期：2026-06-11

## 背景

Hermes Agent 当前是一个大型混合技术栈项目：

- Python 承载核心 agent、CLI、gateway、工具、providers、skills/plugins 机制。
- Electron/React 承载桌面应用 UI。
- Node 生态承载桌面构建、TUI、Web、browser tools 与若干 sidecar。
- 仓库内已有一个 Tauri/Rust bootstrap installer，但它不是核心运行时。

当前桌面包的关键发布行为是：打包产物主要携带 Electron shell，首次启动时再把 Hermes Agent runtime
安装到 `HERMES_HOME`。CLI/服务器路径则依赖 Python、uv、venv、Git 等安装链。用户目标是尽量减少发布版
需要用户预装的依赖，但不减少现有功能；可以把必要 runtime 和依赖资源放进发布包或二进制资源中。

本次要解决的核心问题是安装时依赖链过于庞杂：安装过程需要探测和准备 Python、uv、venv、Git、ripgrep、
Node/native modules、平台 shell 等组件，失败点多，耗时长，卸载时也难以明确哪些资源属于 Hermes 管理。
Rust 化的优先方向应服务于这个问题：减少安装时外部依赖、减少首次启动下载/编译/探测步骤，并让安装、
更新、卸载更快、更稳定、更容易解释，而不是追求一次性把业务逻辑全部改成 Rust。

## 目标

1. 三个平台（Windows、macOS、Linux）都形成可落地的自包含发布路线。
2. 用户在没有预装 Python、uv、Git、ripgrep 的环境里，也能启动核心 CLI/desktop 能力。
3. 保留现有功能面：CLI、desktop、gateway、skills、plugins、lazy deps、providers、更新机制。
4. 用 Rust 优先替换发布边界层，而不是一次性重写核心 agent。
5. 安装和卸载路径由 Rust 管理器统一编排，减少脚本分支和平台探测差异。
6. 每个阶段都可回退到现有 Python 安装链，避免发布路径单点失败。

## 非目标

1. 不在第一阶段重写核心对话循环、provider adapters、gateway 平台适配或 skills/plugin 机制。
2. 不在第一阶段用 Tauri 替换 Electron desktop。
3. 不强行把所有 optional/lazy dependencies 全量内置到每个发布包。
4. 不为了“纯 Rust”牺牲现有功能、平台覆盖或插件兼容性。
5. 不删除、禁用或弱化原版本已有功能入口；只允许新增安装、修复、卸载、离线诊断等管理能力。

## 推荐方案

采用“发布包自包含优先，Rust 只做边界层”的主线方案。

Rust 负责：

- 安装、更新、卸载编排；
- runtime 与资源定位；
- 首次启动解包；
- manifest/stamp 校验；
- Python venv 或等价运行目录创建；
- 本地 wheelhouse 安装；
- Git/ripgrep/ffmpeg 等工具探测与路径注入；
- 子进程启动、退出码归一、日志落盘；
- 更新前置检查和回退编排；
- 管理资源清单，确保卸载只删除 Hermes 拥有的文件。

Python 继续负责：

- agent 主循环；
- CLI 命令语义；
- gateway 与平台 adapters；
- tools、providers、skills、plugins；
- lazy dependency allowlist 与功能级安装策略。

Electron/React 继续负责：

- 桌面窗口；
- renderer UI；
- 与 dashboard/backend 的现有通信路径；
- 现有 native node modules 的加载。

## 阶段路线

### 阶段 0：依赖矩阵与发布资源清单

输出每个平台的发布资源矩阵，至少包含：

- Python runtime 版本与来源；
- core dependencies 对应 wheelhouse；
- uv 是否仍作为内部安装器携带；
- Git、ripgrep、ffmpeg、bash/MinGit 等工具是否内置；
- Electron native resources 与 `node-pty` 资源；
- agent source snapshot、install scripts、lock/stamp 文件；
- optional/lazy dependencies 的联网边界。

阶段 0 的结果是一个机器可读 manifest 草案，后续 Rust launcher 和 Electron bootstrap 都以它为契约。

### 阶段 1：Rust 安装/卸载管理器与自包含启动器

新增 Rust manager/launcher，作为现有 Python/Electron 启动链前面的薄包装层。它不改变 Python API，只负责把
运行环境准备到可启动状态，并接管安装、更新、卸载中最容易出错的平台边界。

核心职责：

- 解析应用资源目录；
- 读取 bundled manifest；
- 检查 `HERMES_HOME` 下 runtime 是否已安装且 stamp 匹配；
- 必要时从发布包资源解包 Python runtime、agent snapshot、wheelhouse 和工具二进制；
- 生成 installed-files manifest，记录 Rust manager 创建或解包的每个路径；
- 设置环境变量和 `PATH`；
- 启动 `hermes_cli.main:main`、`run_agent:main` 或 desktop backend；
- 提供 `install`、`repair`、`uninstall`、`doctor` 等管理子命令；
- 记录结构化日志，失败时给出明确错误和回退建议。

Rust launcher 先通过子进程边界调用 Python，避免第一阶段引入 PyO3 ABI 和 Python extension 打包风险。

### 阶段 2：离线 core wheelhouse

为 core dependencies 建立平台相关 wheelhouse：

- Windows x64/arm64；
- macOS x64/arm64；
- Linux x64/arm64，必要时区分 glibc/musl。

首次启动时优先从本地 wheelhouse 创建运行环境，不访问 PyPI。provider、voice、messaging、matrix、browser
等重依赖继续保留 lazy install。对于断网环境，核心功能应可启动；首次使用需要联网依赖的功能要返回清晰错误。

阶段 2 的重点是减少安装时依赖，而不是减少功能：

- core wheels 必须来自发布包或随包缓存；
- 不在安装阶段解析 optional extras；
- 不在安装阶段编译 native extensions；
- 不在安装阶段安装用户未启用的平台 SDK；
- lazy deps 继续按功能首次使用安装，并保留现有 allowlist 和安全开关。

### 阶段 3：桌面发布包改造

保留 Electron desktop，但改变默认 first-launch 体验：

- 打包时把 agent snapshot、install script、runtime manifest、core wheelhouse 放入 `extraResources`。
- `bootstrap-runner.cjs` 或后续 Rust bridge 优先使用本地资源。
- 只有资源缺失、用户主动更新、或 manifest 允许在线刷新时，才访问 GitHub/PyPI。
- 现有 GitHub pinned install script 下载路径保留为回退路径。

这一步直接改善用户安装依赖：发布包变大，但首次启动更可预测。

### 阶段 4：可选 Rust 化轨道

后续只迁移边界稳定、测试清晰、收益明确的模块：

- 安装/更新编排；
- 依赖探测；
- 解压、校验、stamp 管理；
- 进程树管理；
- 文件索引和 ignore 规则；
- 部分配置/schema 校验；
- 发布包诊断命令。

暂不迁移：

- 核心 agent 对话循环；
- provider adapters；
- gateway 平台适配；
- skills/plugin 发现与执行机制；
- model tools schema 生成路径。

## 发布依赖策略

发布包依赖分三层：

1. 必须内置：启动核心 CLI/desktop 所需 runtime、core wheels、agent snapshot、基础工具。
2. 建议内置：Git、ripgrep、平台必需 PTY/native resources、基础 ffmpeg 能力。
3. 保持 lazy：大型、平台差异强、按功能使用的依赖，例如 faster-whisper、matrix E2EE、部分 browser/voice
   后端和第三方平台 SDK。

这种分层保留现有功能，但避免把所有用户不会使用的重依赖塞进每个发布包。

安装阶段只处理第一层和必要的第二层。第三层不得阻塞首次安装，也不得让安装器因为用户没有使用的功能而失败。

## 硬约束

1. 不删减原版本已有功能，只允许增加安装、修复、卸载、离线诊断等管理能力。
2. 每个阶段都保留回退路径，不能让新发布链成为唯一可用路径。
3. 卸载不能误删用户数据，只能删除明确归属 Hermes 管理的资源。
4. 安装优化不能绕过现有 lazy deps allowlist、安全开关和版本锁定策略。

## 功能兼容性准入

发布包改造必须通过功能兼容性准入：

- 原版本已有 CLI 命令不能删除；
- 原版本已有 desktop 入口和 onboarding 流程不能删除；
- 原版本已有 gateway 平台适配不能因为安装器改造失效；
- 原版本已有 skills/plugins 发现路径不能改变语义；
- 原版本已有 lazy deps 功能必须仍能在首次使用时安装或给出等价错误；
- 原版本已有更新和 doctor 诊断路径必须保留或提供等价入口。

任何功能差异都应视为回归，除非明确属于新增能力或错误信息改善。

## 卸载策略

Rust manager 维护 Hermes 托管资源清单：

- runtime 目录；
- agent snapshot；
- venv 或等价运行目录；
- bundled tools；
- desktop bootstrap cache；
- manager 写入的 stamp、manifest、日志索引。

卸载流程分为三种模式：

1. lite：删除托管 runtime 和缓存，保留用户配置、sessions、skills、memories。
2. full：删除托管 runtime、缓存和 Hermes 配置目录，但需要二次确认。
3. repair-clean：只删除损坏 runtime 和安装 stamp，下次启动重新从发布包资源恢复。

卸载器只能删除 installed-files manifest 中记录的托管路径，不能根据宽泛目录猜测删除用户数据。

## 错误处理

Rust launcher 和 bootstrap 层必须提供明确错误分类：

- bundled manifest 缺失或版本不匹配；
- runtime 解包失败；
- wheelhouse 缺少当前平台 wheel；
- Python runtime 不可执行；
- venv 创建失败；
- core dependency 安装失败；
- optional/lazy dependency 在断网环境下不可安装；
- 回退到在线安装失败；
- 卸载清单缺失或路径越界。

每类错误都应写入日志，并尽量给出用户可执行的恢复路径。

## 验证标准

每个阶段完成前至少验证：

- Windows、macOS、Linux fresh profile 启动；
- 无系统 Python/uv/Git/ripgrep 时核心 CLI 可启动；
- desktop fresh install 能进入 onboarding 或主界面；
- 断网情况下核心功能不访问 PyPI/GitHub；
- optional/lazy 功能首次使用时错误清晰，不崩溃；
- 现有 Python 单测相关路径通过；
- `npm run test:desktop:all` 仍通过；
- installer smoke test 覆盖新旧路径；
- installer smoke test 覆盖 install、repair、lite uninstall 和 full uninstall；
- 更新失败可以回退到上一个可用 runtime。

## 主要风险

1. 发布包体积增加：这是自包含发布的直接代价，需要用分层资源和 lazy deps 控制。
2. 平台 wheel 差异：Linux glibc/musl、macOS universal、Windows native wheels 都需要单独验证。
3. 卸载误删风险：必须依赖 installed-files manifest 和路径边界检查，不允许宽泛递归删除。
4. Electron 与 Rust 双 bootstrap 复杂度：必须用 manifest 作为单一契约，避免 JS/Rust 各自推断环境。
5. lazy deps 断网体验：不能假装所有功能离线可用，应明确哪些功能需要联网安装。
6. 过早 PyO3 化：会引入 ABI、打包和崩溃诊断复杂度，第一阶段应避免。

## 后续实施计划入口

本设计通过后，实施计划应从阶段 0 开始：

1. 生成发布资源矩阵。
2. 定义 bundled manifest schema。
3. 为 Windows 做首个 Rust manager/launcher proof-of-concept。
4. 实现 installed-files manifest 和 lite uninstall。
5. 接入 desktop `extraResources` 的本地资源优先路径。
6. 扩展到 macOS 和 Linux。
