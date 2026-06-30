# Agent 工具模式

`rjs_lite` 可以作为 AI Agent 工作流中的本地 JavaScript 执行工具。Agent 提交一段短小 JavaScript 片段，Runtime 执行受支持的子集，然后进程返回一个结构化 JSON 结果。

这是本地 CLI/library 契约。它不是 MCP server、网络服务，也不提供安全沙箱。

## CLI 契约

```bash
cargo run -- --agent-eval "let x = 1 + 2; print(x); x;"
```

示例输出：

```json
{"ok":true,"request_id":"local","value":"3","value_type":"number","output":["3"],"output_truncated":false,"error_kind":null,"error":null}
```

只要工具进程自身成功运行，Agent 模式总是成功退出。JavaScript 解析或运行时失败会以 `ok:false` 编码在 JSON 结果中。

Agent 模式会应用默认运行时限制：源代码上限为 64 KiB，捕获输出上限为 256 行，执行会在 100,000 个解释器步数后停止，嵌套用户自定义函数调用会在 16 帧后停止。

## 结果字段

- `ok`：JavaScript 执行是否成功。
- `request_id`：执行上下文标识符。CLI 默认值是 `local`。
- `value`：最终求值结果的字符串形式，失败时为 `null`。
- `value_type`：运行时类型名称，例如 `number`、`string`、`boolean`、`object`、`undefined` 或 `function`。`null` 遵循 JavaScript 行为，类型报告为 `object`。
- `output`：捕获到的 `print(value)` 输出行。
- `output_truncated`：输出是否超过配置的行数限制。
- `error_kind`：失败时的稳定错误分类，成功时为 `null`。当前取值包括 `lex`、`parse`、`runtime`、`source_limit`、`step_limit` 和 `call_depth_limit`。
- `error`：失败时的诊断字符串，成功时为 `null`。

## Rust API

```rust
let result = rjs_lite::run_agent_tool("let x = 1 + 2; x;");
println!("{}", result.to_json());
```

用于自定义执行元数据和限制：

```rust
let runtime = rjs_lite::AgentRuntime::new(rjs_lite::ExecutionContext {
    request_id: "agent-call-1".to_string(),
    limits: rjs_lite::RuntimeLimits {
        max_source_bytes: 64 * 1024,
        max_output_lines: 256,
        max_execution_steps: 100_000,
        max_call_depth: 16,
    },
    host_functions: vec![rjs_lite::HostFunction {
        name: "print".to_string(),
        arity: 1,
        description: "Capture one value into output".to_string(),
    }],
});

let result = runtime.run("print(42);");
```

`host_functions` 目前用于描述预期的宿主能力表面。MVP 解释器只暴露 `print(value)`；自定义宿主函数注册机制尚未实现。

## 限制

- 只支持已文档化的 MVP JavaScript 子集。
- Agent 模式会在配置的解释器步数限制后停止执行。
- Agent 模式会在配置的调用深度限制后停止执行。
- 不声称具备完整 ECMAScript 兼容性。
- 尚不声称具备 test262 通过率覆盖。
- 不提供安全沙箱。
- 不向 JavaScript 暴露文件 API、网络 API、进程 API 或 OS API。
- 不封装 Node.js、Deno、QuickJS、Boa、V8 或其他 JS 引擎。

## Agent 使用建议

此工具适合短生命周期片段，例如：

- 算术和控制流检查
- 当前语法支持的小型数据转换逻辑
- 宿主工具调用前的短小胶水代码实验
- 确定性的本地计算

避免将它用于不可信代码隔离、浏览器 API、网络访问、npm packages、大型脚本或完整 ECMAScript 兼容性测试。
