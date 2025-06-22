# 单例任务 (Singleton Task)

[![CI](https://github.com/ZR233/singleton-task/workflows/CI/badge.svg)](https://github.com/ZR233/singleton-task/actions)
[![Crates.io](https://img.shields.io/crates/v/singleton-task.svg)](https://crates.io/crates/singleton-task)
[![Documentation](https://docs.rs/singleton-task/badge.svg)](https://docs.rs/singleton-task)
[![License](https://img.shields.io/badge/license-MulanPSL--2.0-blue.svg)](LICENSE)

## 描述

本项目实现了一个基于 Rust 的异步任务管理系统，主要功能包括任务状态管理、任务通信以及任务调度。核心特性如下：

- **单例任务管理**：通过 `SingletonTask` 结构体，确保一个任务在整个系统中只有一个实例运行。
- **任务状态跟踪**：任务的状态变化可通过 `Context` 进行跟踪和控制。
- **异步支持**：使用 `async_trait` 和 `Future` 实现异步任务处理。
- **跨线程通信**：通过自定义的 `task_channel` 实现任务间的同步和异步通信。

## 安装与使用

### 构建项目

确保你已安装 [Rust 工具链](https://www.rust-lang.org/tools/install)，然后运行以下命令：

```bash
cargo build
```

### 运行示例

进入项目目录并运行示例代码：

```bash
cargo run --example task
```

## 项目结构

- `src/lib.rs`：定义主要的 trait 和结构体，包括 `Task`, `TaskBuilder`, `SingletonTask` 等。
- `src/context.rs`：实现任务上下文管理，支持状态变更和异步等待。
- `src/task_chan.rs`：提供任务间通信的通道机制。
- `examples/task.rs`：展示如何使用单例任务系统。
- `tests/test.rs`：包含异步任务控制逻辑的测试用例。

## 测试

运行所有测试：

```bash
cargo test
```

运行多线程测试：

```bash
cargo test --test test -- --test-threads=1
```

## 贡献

欢迎贡献代码！请确保提交 PR 前运行下列检查：

```bash
# 格式化代码
cargo fmt

# 运行 Clippy 检查
cargo clippy

# 运行测试
cargo test
```

## 许可证

本项目采用 MulanPSL-2.0 许可证 - 详见 [LICENSE](LICENSE) 文件。
