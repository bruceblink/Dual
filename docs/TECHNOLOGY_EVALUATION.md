# Dual 技术路线说明

## 结论

`Dual` 当前保留 **Java 21 + Processing** 作为迁移基线，同时在 `feature/macroquad-migration` 分支新增 **Rust + Macroquad** 客户端和 **Rust + Tokio** Relay。Rust 迁移采用并行渐进方式，先交付 Windows 本地版，再完成联机互操作；Java 实现保留到 Rust 联机验收和切换评估完成。迁移细节见 [Macroquad 迁移计划](01-macroquad-migration-plan.md)。

`dual-game-client` 是另一款独立游戏。它使用 Godot，不代表 `Dual` 的技术升级，也不是 `Dual` 的正式版、重制版或替代工程。两个项目只是在同一开发环境中并存，不能据此合并产品定位或开发计划。

## 当前技术栈

| 范围 | 技术 | 职责 |
| --- | --- | --- |
| 游戏客户端基线 | Java 21 + Processing 4 | 迁移期间的既有游戏循环、渲染、输入、角色、箭矢、AI、状态与表现。 |
| 构建与测试 | Gradle + JUnit 5 | 编译、单元测试、依赖与多模块构建。 |
| 联机服务 | Java TCP Relay | 连接两名玩家并转发输入、回合结果和再战请求，不维护权威游戏状态。 |
| Windows 发布基线 | `jpackage` + WiX 3 | 生成 Java 客户端安装包，迁移期间保留。 |
| Rust 客户端 | Rust + Macroquad | 无 JVM 的 Windows 游戏客户端，先覆盖本地玩法，再接入联机。 |
| Rust Relay | Rust + Tokio | 与 Java Relay 保持固定帧互操作的 TCP Relay。 |

## 技术边界

- Java 基线玩法继续按现有开发计划演进；Rust 迁移按 [Macroquad 迁移计划](01-macroquad-migration-plan.md) 单独验收。
- Java Relay 当前只转发客户端输入、回合结果和再战请求，不维护权威游戏状态；Rust Relay 必须保持同一报文和行为边界。
- 如果未来需要房间码、匹配、断线恢复或服务端权威判定，应先由 `Dual` 的玩法和发布需求驱动设计，再独立评估是否升级现有 Java 服务端。
- 另一个 Godot 游戏的场景、脚本、协议、资产、版本和里程碑均不属于 `Dual`。

## 演进原则

1. 优先验证游戏是否更好玩，再决定是否引入新的基础设施。
2. 保留短弓击退、长弓致命和箭矢抵消这一核心战斗关系。
3. 新玩法应先在本地人机模式形成完整闭环，再验证本地双人或网络同步需求。
4. 每项技术升级都应解决可复现的问题，并提供与风险相匹配的自动化测试或运行验证。

## 与其他项目的关系

| 项目 | 定位 | 与 `Dual` 的关系 |
| --- | --- | --- |
| `Dual` | Java + Processing 双人弓箭对战游戏 | 当前项目，继续独立开发。 |
| `dual-game-client` | Godot 开发的另一款游戏 | 产品和工程均独立，不是 `Dual` 的后续版本。 |

因此，任何把 `Dual` 描述为“冻结 Demo”，或把 Godot 项目描述为其“正式游戏”的文档，均属于错误描述。
