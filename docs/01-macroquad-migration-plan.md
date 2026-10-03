# Dual Macroquad 迁移开发计划

## 术语表与命名约定

| 规范名称 | English / Acronym | 本计划中的职责边界 | 不代表什么 |
| --- | --- | --- | --- |
| Java 基线客户端 | Java baseline client | 当前 Java 21 + Processing 客户端，用于行为对照、回滚和迁移期间的功能参考。 | 不代表迁移后的长期客户端。 |
| Rust 客户端 | Rust client | 使用 Rust + Macroquad 实现的 Windows 优先桌面游戏客户端。 | 不包含 Relay 服务端。 |
| Rust Relay | Rust Relay | 使用 Rust + Tokio 实现的 TCP 房间和固定帧转发服务。 | 不维护比赛权威状态。 |
| 确定性模拟器 | deterministic simulation / `dual-sim` | 不依赖窗口、渲染和网络的规则层，接收输入并产生状态快照。 | 不负责绘制、音频、窗口或 TCP。 |
| 协议 crate | protocol crate / `dual-protocol` | Rust 与 Java 之间的报文常量、编码、解码和校验规则。 | 不负责房间或玩法状态。 |
| 行为等价 | behavior parity | 相同种子和相同输入序列下，Rust 规则结果与 Java 基线保持约定一致。 | 不要求源码结构或绘制实现相同。 |
| 本地版发布 | local release | Rust 客户端覆盖演示、人机、本地双人、暂停、设置、两张场地和比赛流程后的首个发布阶段。 | 不表示联机迁移已经完成。 |

正文、代码目录和验收记录统一使用以上名称。`Dual`、Macroquad、Tokio、Processing、TCP、Relay 等固定名称保留标准拼写。

## 1. 目标与范围

本计划把当前 Java 21 + Processing 的 `Dual` 客户端逐步迁移为 Rust + Macroquad 客户端，并同时建立 Rust Relay。迁移期间保留 Java 客户端和 Java Relay，先完成 Rust 本地版，再完成跨实现联机互操作，最后由单独的发布决定切换默认客户端。

目标结果：

- Rust 客户端不依赖 JVM，生成 Windows 原生可执行文件和安装包。
- Rust 规则层不依赖窗口、渲染和网络，能够独立测试和录制回放。
- Rust 客户端保留当前 60 FPS、1280×720 逻辑竞技场、16:9 缩放、演示、人机、本地双人、中央掩体、暂停、设置、比分、再战和反馈表现。
- Rust 客户端实现当前固定网络协议，并先与 Java Relay 互操作，再与 Rust Relay 互操作。
- Java 实现作为迁移期间的对照和回滚基线，不在 Rust 联机验收完成前删除。

明确不在本轮范围内：

- 不实现通用 Processing API 或 Java/Rust FFI。
- 不改变现有网络报文格式来适配 Rust。
- 不在迁移完成前重做玩法数值、战斗规则或产品定位。
- 不把 gpui-kit 作为游戏渲染层；本计划使用 Macroquad 实现游戏窗口和绘制。

## 2. 当前基线

当前仓库为 Java/Gradle 多模块项目：

- 客户端使用 Java 21、Processing 4.5.0 和 JUnit 5。
- Relay 位于 `server/`，使用 Java TCP Socket 转发固定帧。
- 当前客户端约 6,207 行主源码，约 29 个文件直接依赖 Processing 或 `App` 绘制调用。
- 当前协议包括输入、共享种子握手、握手确认、断线、回合结果和再战请求。
- 分支阶段 0 必须运行 `./gradlew test` 和 `./gradlew build` 并保存结果。

Java 基线的产品和规则文档继续有效：

- `GAME_DESIGN.md` 规定玩法和数值意图。
- `GAMEPLAY_IMPLEMENTATION_GUIDE.md` 规定输入和规则实现边界。
- `DEVELOPMENT_PLAN.md` 记录 Java 基线中的桌面试玩和发布条件。

Rust 迁移版本必须重新完成相同范围的桌面验收；headless 结果不能替代真实窗口证据。

## 3. 分支、目录与提交边界

### 3.1 分支

- 基线：最新 `origin/main`，并快进到包含当前完整玩法的最新主线祖先提交。
- 开发分支：`feature/macroquad-migration`。
- 每个阶段通过验证后立即推送到 `origin/feature/macroquad-migration`。
- 不直接修改或推送 `main`。
- Java 客户端和 Relay 在 Rust 联机验收和切换评估完成前不得删除。

### 3.2 Rust 工作区

```text
rust/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── dual-protocol/
│   ├── dual-sim/
│   ├── dual-client/
│   └── dual-relay/
├── fixtures/
│   ├── protocol/
│   └── replay/
└── docker-compose.integration.yml
```

`dual-client` 依赖 `dual-protocol` 和 `dual-sim`；`dual-relay` 依赖 `dual-protocol` 及网络运行时；`dual-sim` 不依赖 Macroquad、Tokio 或窗口库。

每个独立可验收阶段单独提交，标题使用英文 Conventional Commit。测试失败、桌面验收未完成或包含无关修改时不得提交。

## 4. 目标架构

### 4.1 `dual-protocol`

固定以下线格式，字节顺序和长度必须与 Java 实现一致：

- `TYPE_INPUT`：`[type][flags][uint16 aimAngle]`，总长 4 字节。
- `TYPE_START`：`[type][int32 seed]`。
- `TYPE_START_ACK`：单字节。
- `TYPE_DISCONNECT`：单字节。
- `TYPE_ROUND_RESULT`：`[type][round][winner][playerOneWins][playerTwoWins][matchComplete]`，总长 6 字节。
- `TYPE_REMATCH_REQUEST`：`[type][round][matchReset]`，总长 3 字节。

输入 flags 保持 bit 0–5 的移动和武器按键、bit 6 的 `HAS_AIM`，bit 7 必须为 0。没有有效瞄准时角度字段必须为 0；有效角度使用无符号 16 位整圈量化。

公开接口至少包括：

```rust
pub fn encode_input_frame(input: InputFrame) -> [u8; 4];
pub fn decode_input_frame(frame: &[u8]) -> Result<InputFrame, ProtocolError>;
pub fn encode_round_result(result: RoundResult) -> [u8; 6];
pub fn decode_round_result(frame: &[u8]) -> Result<RoundResult, ProtocolError>;
pub fn encode_rematch_request(request: RematchRequest) -> [u8; 3];
pub fn decode_rematch_request(frame: &[u8]) -> Result<RematchRequest, ProtocolError>;
```

使用 Java fixture 建立 byte-for-byte golden vector，覆盖正常帧、边界角度、未知 flags、错误长度、无 `HAS_AIM` 却带角度、重复结果和非法枚举值。

### 4.2 `dual-sim`

规则层使用固定帧推进和显式输入意图：

```rust
pub struct Simulation;
pub struct SimulationConfig;
pub struct PlayerInput;
pub struct PlayerState;
pub struct ArrowState;
pub struct FrameSnapshot;
pub struct MatchScore;

impl Simulation {
    pub fn new(config: SimulationConfig, seed: u64) -> Self;
    pub fn step(&mut self, inputs: [PlayerInput; 2]);
    pub fn snapshot(&self) -> FrameSnapshot;
    pub fn reset_round(&mut self);
    pub fn round_result(&self) -> Option<RoundResult>;
}
```

规则层必须保持 60 FPS、1280×720、Open 与 Central Cover、短弓、长弓、击退、破绽、蓄力、自动锁定、手动瞄准、拦截、战术事件、比分、先胜三回合、回合重置和三档 AI。AI、真人和网络玩家只能生成 `PlayerInput`，不能直接修改模拟状态。

### 4.3 `dual-client`

Macroquad 客户端负责窗口、输入、渲染、音频和网络适配：

- 默认窗口 1920×1080，可缩放；逻辑画布固定 1280×720，保持 16:9 留白。
- 固定步长模拟和渲染解耦；渲染只读取 `FrameSnapshot` 和事件。
- 失焦时清除两名本地玩家输入，并按当前规则暂停。
- TCP 接收放到后台线程或异步任务，主循环不得阻塞等待网络。
- 先实现演示、人机、本地双人、暂停、设置和结果页，再接入联机大厅。
- 使用 `assets/` 或嵌入式资源保存字体、图标和音效；资源缺失必须产生明确启动错误。

渲染模块按场地、玩家、箭矢、粒子、HUD、菜单和结果层拆分，不能修改模拟器的比分、碰撞或武器状态。

### 4.4 `dual-relay`

Rust Relay 使用 Tokio 实现 Java Relay 的行为：两人房间、5 秒握手超时、TCP_NODELAY、共享 seed、固定帧转发、断线通知、非法或截断帧关闭房间。Relay 不解析或修改玩法结果，不保存比赛权威状态。

## 5. 分阶段实施

### 阶段 0：基线冻结

创建 `feature/macroquad-migration`，运行 Java 测试和构建，保存协议字节向量、共享 seed 行为、固定输入序列、关键帧快照、最终比分和当前 Windows app-image 桌面行为。

完成条件：基线命令通过，fixture 已保存，原始 Java 文件未被修改。

### 阶段 1：Rust 工作区和协议

建立 Cargo workspace 和锁文件，实现 `dual-protocol` 的类型、编码、解码和错误边界；用 Java fixture 做 golden vector；确认 Java 回归通过。

完成条件：Rust fmt、Clippy、workspace tests 和 Java 回归通过；提交 `feat: add rust protocol workspace` 并推送。

### 阶段 2：确定性规则层

按常量/场地、输入/移动、箭矢/碰撞、武器状态、比分/回合、事件、AI/本地双人顺序实现 `dual-sim`。每一步添加规则测试，并用固定 seed 和回放输入比较 Java/Rust 的关键帧和最终结果。

完成条件：关键 Java 规则场景在 Rust 通过，至少一组完整比赛回放一致；提交 `feat: add deterministic rust simulation` 并推送。

### 阶段 3：Macroquad 本地客户端

实现固定步长循环、画布变换、键鼠输入、失焦清理、暂停、场地、玩家、箭矢、粒子、HUD、反馈、演示、模式选择、人机、本地双人、设置、音量和再战。

完成条件：Windows 真实窗口完成演示、人机先胜三回合、本地双人、中央掩体、暂停、设置、失焦恢复和再战；提交 `feat: add macroquad local client` 并推送。

### 阶段 4：Rust 本地版发布

构建 MSVC release 可执行文件和 WiX 3 安装包；安装包不得包含 JRE、JDK 或 `runtime/`；在无 `java.exe`、无 `JAVA_HOME` 的干净 Windows 环境验证安装、启动、升级和卸载。Java 客户端继续作为回滚基线。

完成条件：本地模式真实窗口验收和无 JVM 启动通过；提交 `chore: package rust windows client` 并推送。

### 阶段 5：Rust Relay 和联机

依次验证 Rust 客户端 ↔ Java Relay、Java 客户端 ↔ Rust Relay、Rust 客户端 ↔ Rust Relay；覆盖 seed、输入、结果、再战、延迟、断线、超时、重复和截断帧；使用本机 Docker 完成集成测试并记录镜像、端口、启动和清理状态。

完成条件：三种互操作组合和真实双客户端试玩通过；提交 `feat: add rust relay` 并推送。

### 阶段 6：最终切换评估

确认本地和联机真实窗口验收、固定输入回放、协议互操作、无 JVM 安装包、Computer Use 证据和 Java 回滚包全部齐全，再提交切换评估。此阶段不自动删除 Java 客户端、Java Relay 或迁移 fixture。

## 6. 验证和完成定义

Rust 每次提交前，从 `rust/` 根目录运行：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Java 回归从仓库根目录运行：

```powershell
./gradlew test
./gradlew build
```

本机 Docker 集成测试使用：

```powershell
docker compose -f rust/docker-compose.integration.yml up --build -d
cargo test --workspace
docker compose -f rust/docker-compose.integration.yml down --volumes --remove-orphans
```

Docker 不可用时集成测试标记为未完成，不用 mock 或静态 fixture 冒充通过。Computer Use 优先用于真实窗口流程；不可用时必须记录替代证据和未覆盖的窗口行为。

本迁移只有在 Rust 客户端覆盖本地和联机功能、Rust Relay 完成三种组合互操作、规则/协议/集成/Windows 安装/真实窗口证据齐全且无 JVM 安装包在干净 Windows 工作后才算完成。部分阶段必须明确标记为未完成项。

## 7. 风险与回滚

- 固定 Macroquad 和锁文件，升级单独提交并重新验证。
- Java/Rust 差异定位到第一处不同帧，不放宽断言掩盖差异。
- 固定步长设置最大补偿帧数，避免窗口卡顿无限追帧。
- 网络读取不在渲染线程执行；关键结果帧不能静默丢弃。
- 启动时检查必需资源，缺失时输出明确错误。
- Rust 发布失败时回退到 Java 客户端和 Java Relay，保留迁移分支的阶段提交。

## 8. 实施状态

### 2026-10-02

- 阶段 1 已提交并推送：Rust 协议 crate 覆盖固定帧编码、解码和 Java 格式字节向量。
- 阶段 2 已实现确定性规则基础，包括固定帧、移动、短弓、长弓、掩体、碰撞、比分、再战和三档 seeded AI。Java/Rust 固定输入回放比较仍未完成，因此阶段 2 尚未验收完成。
- 阶段 3 当前工作树包含 Macroquad 客户端基础：等比画布映射、固定步长、键鼠瞄准、AI 和本地双人输入、两种场地、暂停、失焦清理、比分与再战。演示、设置/音量、音效和完整视觉反馈仍未实现。
- Computer Use 在重置后重新初始化并盘点应用，返回的应用列表为空；运行时未提供 `computer.launch_app` 和 `listWindows`，无法绑定或截取 Macroquad 窗口。系统进程检查曾返回窗口标题 `Dual` 和 `Responding=True`，但这不能证明真实画面或鼠标/键盘流程通过。此次仅有 viewport、输入边沿和焦点事件单元测试等 headless 证据。
- Rust 验证命令：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 和 `cargo build --workspace` 均通过。协议、规则和客户端共 31 个单元测试通过。
- Docker 集成测试不适用：当前切片没有外部服务。Java Gradle 回归未在此次 Rust 切片中运行。

阶段 2 和阶段 3 保持未完成状态，直到固定输入回放及本地窗口流程按本计划完成验收。

### 2026-10-03

- 阶段 3 新增设置覆盖层：使用鼠标或 `O` 打开，以 `+`/`-` 或按钮调整 100%、50%、静音三档音量，以 `M` 或按钮静音，使用 `Escape` 或 Back 按钮返回；设置期间及关闭菜单的输入帧冻结模拟。
- `dual-sim` 在长弓首次蓄满时产生单帧 `LongbowChargeReady` 事件；客户端用独立后台音频线程尝试播放 80ms、880Hz 提示音，音量随设置变化。音频设备初始化失败时记录错误并保持静音运行。真实设备上的可听输出尚未单独验证。
- Windows 真实窗口通过 Computer Use 验证演示画面、鼠标打开设置、音量从 100% 调至 50% 和静音、返回原回合结果页。此次只验收设置交互；人机完整三回合、本地双人、暂停/恢复、两张场地和失焦恢复仍未全部完成。针对回合提示与音量文字重叠问题，设置层改用不透明遮罩。
- Rust 验证：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 和 `cargo build --workspace` 均通过；协议、规则和客户端共 37 个单元测试通过。
- Docker 集成测试不适用：此切片没有外部服务。Java Gradle 回归未运行：未修改 Java 基线。

阶段 3 仍未完成；固定输入 Java/Rust 回放比较、剩余本地窗口流程、完整反馈与真实音频输出验收保持待办。
