# rjs_lite：面向 AI Agent 的轻量级 JavaScript 执行 Runtime

`rjs_lite` 是一个面向 AI Agent 工作流的轻量级脚本执行 Runtime。它以 Rust 原生 JavaScript 子集作为第一执行语言，服务于短生命周期、高频工具调用、低依赖本地执行等场景。

本项目不是 QuickJS、Boa、V8、Node.js 或 Deno 的套壳封装。当前代码已经实现自研的词法分析器、语法分析器、AST、运行时值模型、词法作用域环境、树遍历解释器和面向 Agent 的结构化执行结果 API，为后续扩展 ECMAScript 兼容性、test262 测试覆盖率和性能优化打基础。

## 项目目标

赛题名称为“面向 AI Agent 的轻量级执行引擎”。本项目将其理解为：给 AI Agent 提供一个本地、轻量、可嵌入的 JavaScript 执行工具，让 Agent 可以把临时脚本交给 Runtime 执行，并获得结构化结果。

本项目当前定位为可运行的 MVP 引擎骨架，重点先完成三件事：

1. 建立原生 JS 引擎核心架构。
2. 支持基础 JS 语法执行，能通过自带单元测试和 CLI 测试。
3. 提供 Agent 可直接消费的结构化 JSON 执行结果。
4. 为 test262 接入、对象模型、标准库和性能优化预留清晰演进路线。

## 当前支持能力

已支持：

- 数字、字符串、布尔值、`null`、`undefined` 字面量
- `let` 和 `const` 变量声明
- 对可变绑定进行赋值
- 算术、比较、相等、逻辑和一元运算符
- 代码块、`if`、`else`、`while`、`break` 和 `continue`
- 函数声明、函数调用和 `return`
- 最小宿主内置函数：`print(value)`
- CLI 执行内联源码或 `.js` 文件
- `--agent-eval` Agent 工具模式，输出结构化 JSON 结果
- `AgentRuntime`、`ExecutionContext`、`RuntimeLimits` 和 `run_agent_tool` Rust API
- lexer、parser、interpreter 和 CLI 的基础测试

暂不支持：

- 对象和数组完整语义
- 原型链和 ECMAScript 标准库对象
- class、module、async、generator、promise、regexp、symbol、BigInt
- 完整 JS 类型隐式转换规则
- test262 自动化测试框架
- JIT、字节码虚拟机和高级优化流水线

## 快速开始

执行内联 JS：

```bash
cargo run -- -e "let x = 1 + 2 * 3; print(x);"
```

预期输出：

```text
7
```

执行 JS 文件：

```bash
cargo run -- examples/demo.js
```

查看帮助：

```bash
cargo run -- --help
```

作为 AI Agent 工具调用：

```bash
cargo run -- --agent-eval "let x = 1 + 2; print(x); x;"
```

预期输出一行 JSON：

```json
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error":null}
```

示例程序：

```javascript
function fib(n) {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

print(fib(8));
```

## 架构设计

执行流程：

```text
JS 源码 -> lexer -> tokens -> parser -> AST -> interpreter -> Value
```

核心模块：

- `src/lexer.rs`：手写词法分析器，负责 token 生成、注释处理和错误位置记录
- `src/parser.rs`：手写递归下降语法分析器，支持表达式优先级
- `src/ast.rs`：程序、语句和表达式 AST 数据结构
- `src/value.rs`：运行时值模型和显示逻辑
- `src/interpreter.rs`：词法作用域、控制流、函数调用和内置函数执行
- `src/agent.rs`：Agent 执行上下文、基础限制、宿主函数元信息和结构化工具结果
- `src/main.rs`：无第三方依赖的命令行入口

## Agent 工具模式

从 AI Agent 视角看，`rjs_lite` 可以作为一个本地 JS 执行 tool。Agent 将短小 JavaScript 片段传给 `--agent-eval`，Runtime 执行后返回一个 JSON 对象，包含是否成功、最终值、`print` 输出和错误信息。

字段说明：

- `ok`：JS 执行是否成功
- `request_id`：执行上下文 ID，CLI 默认是 `local`
- `value`：最终返回值的字符串形式，失败时为 `null`
- `value_type`：运行时类型，例如 `number`、`string`、`boolean`
- `output`：通过 `print(value)` 捕获的输出行
- `output_truncated`：输出是否被限制截断
- `error`：失败时的诊断信息，成功时为 `null`

Rust 侧可以直接调用：

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

更多契约说明见 `docs/agent-tool-zh-cn.md`。

## 非套壳说明

项目当前没有依赖任何外部 JS 引擎，也没有通过子进程调用 Node.js、Deno、QuickJS 或 Boa。

当前解释执行链路全部在 Rust 项目内部完成：源码由本项目 lexer 切分为 token，再由 parser 生成 AST，最后由 interpreter 执行 AST 并返回运行时值。

Agent 工具层同样不调用外部 JS 引擎。`--agent-eval` 只是把本项目内部解释器的结果包装为 Agent 更容易解析的 JSON。

## 面向赛题的后续路线

功能完整度方向：

1. 扩展对象、数组、属性访问和原型链。
2. 补齐常用标准库对象和函数。
3. 接入 test262 runner，记录测试通过率和失败分类。
4. 优先覆盖 AI agent 场景中高频使用的 JS 子集。
5. 扩展 HostFunction 注册机制，让 Agent 能显式挂载安全可控的宿主能力。

性能 benchmark 方向：

1. 增加算术、循环、函数调用、对象访问等微基准。
2. 在解释器稳定后引入字节码编译和 VM 执行层。
3. 参考 JetStream 类 workload 设计短时执行性能测试。
4. 增加 Agent workload：冷启动、重复短脚本、JSON 数据转换、宿主函数调用。
5. 优化启动时间、内存占用和解释器热路径。

创新性方向：

1. 保持原生 Rust 实现，避免套壳。
2. 针对短生命周期脚本执行优化，而不是复制浏览器重量级引擎假设。
3. 通过 `AgentRuntime` 和 `ExecutionContext` 把 JS 引擎包装成 Agent 可调用工具。
4. 探索 agent 场景下的宿主 API、沙箱和资源限制能力。

## 开发与验证

格式化代码：

```bash
cargo fmt
```

检查编译：

```bash
cargo check
```

运行测试：

```bash
cargo test
```

如果本地安装了 Clippy，可额外执行：

```bash
cargo clippy -- -D warnings
```

## 提交说明

本仓库适合直接上传到 GitLab/Gitee。建议保留 `Cargo.lock`，因为当前项目是可执行程序。

不要提交以下内容：

- `target/`
- IDE 配置目录，例如 `.idea/`、`.vscode/`
- 日志文件和本地临时文件
- 自动化工具运行状态目录，例如 `.omo/`

## 当前阶段说明

当前版本已经能作为比赛项目的初始工程提交，但还不是完整 ECMAScript 引擎。后续开发重点应放在 test262 子集推进、对象模型、标准库、性能基准和字节码执行层。
