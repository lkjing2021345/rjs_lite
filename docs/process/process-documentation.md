# rjs_lite 过程性文档

## 一、项目概述

**项目名称：** rjs_lite —— 面向 AI Agent 的轻量级 JavaScript 执行 Runtime

**项目类型：** Rust 原生 JavaScript 子集引擎（非套壳实现）

**版本：** 0.1.0

**技术栈：** Rust（edition 2024），零外部依赖

**项目地址：** 本仓库

---

## 二、设计思路

### 2.1 项目定位

赛题名称为"面向 AI Agent 的轻量级执行引擎"。本项目的核心理解是：为 AI Agent 提供一个本地、轻量、可嵌入的 JavaScript 执行工具，让 Agent 可以将临时脚本交给 Runtime 执行，并获得结构化结果。

### 2.2 核心架构设计

执行流水线：

```
JS 源码 -> lexer -> tokens -> parser -> AST -> interpreter -> Value
```

采用经典的编译前端四阶段设计，但不生成字节码或机器码，而是直接由树遍历解释器执行 AST。

### 2.3 模块划分

| 模块 | 文件 | 职责 |
|------|------|------|
| 词法分析器 | `src/lexer.rs` | 手写字符级词法分析，生成 Token 序列 |
| 语法分析器 | `src/parser.rs` | 递归下降解析，支持表达式优先级 |
| 抽象语法树 | `src/ast.rs` | AST 数据结构定义 |
| 运行时值 | `src/value.rs` | 运行时值模型、类型系统、显示逻辑 |
| 解释器 | `src/interpreter.rs` | 词法作用域、控制流、函数调用、内置函数 |
| Agent 接口 | `src/agent.rs` | Agent 执行上下文、资源限制、结构化结果 |
| 主入口 | `src/main.rs` | CLI 入口、REPL、Agent 工具模式 |
| 错误处理 | `src/error.rs` | 错误类型、位置信息 |
| Token 定义 | `src/token.rs` | Token 类型和关键字映射 |

### 2.4 关键设计决策

1. **零外部依赖**：不使用任何第三方 Rust crate，完全依靠 Rust 标准库实现。这确保了项目轻量、可移植、审计简单。

2. **手写实现而非套壳**：自研 lexer、parser、AST、解释器，不包装 QuickJS、Boa、V8、Node.js 或 Deno。

3. **树遍历解释器而非字节码 VM**：MVP 阶段优先保证正确性和可扩展性，后续可引入字节码编译层。

4. **Agent 优先的接口设计**：`--agent-eval` 模式输出结构化 JSON，包含 `ok`、`value`、`value_type`、`output`、`error_kind`、`error` 等字段，方便 Agent 直接消费。

5. **资源限制机制**：内置源码大小限制（64KB）、执行步数限制（100K）、调用深度限制（16层）、输出行数限制（256行），防止恶意或失控脚本。

---

## 三、实现重点

### 3.1 词法分析器（`src/lexer.rs`）

- 字符级扫描，支持 Unicode 字符
- 处理单行注释 `//` 和多行注释 `/* */`
- 支持数字字面量（整数和小数）、字符串字面量（单引号和双引号，含转义序列）
- 标识符和关键字识别（`keyword_or_identifier` 函数）
- 运算符消歧义：`+`/`++`/`+=`、`-`/`--`/`-=`、`=`/`==`/`===` 等
- 错误位置记录（行号、列号、字节偏移）

### 3.2 语法分析器（`src/parser.rs`）

- 递归下降解析，支持运算符优先级层次：
  `assignment → conditional → logical_or → logical_and → equality → comparison → term → factor → unary → call → primary`
- 语句类型：`VarDecl`、`FunctionDecl`、`Return`、`Throw`、`Try`、`If`、`While`、`For`、`Switch`、`Block`、`Break`、`Continue`、`Expr`
- 表达式类型：`Number`、`String`、`Bool`、`Null`、`Undefined`、`This`、`Identifier`、`Array`、`Object`、`Function`、`Unary`、`Typeof`、`Conditional`、`Binary`、`Assign`、`CompoundAssign`、`Update`、`Call`、`New`、`Member`、`Index`
- 左值检查（`is_assignable`）：只有标识符、成员访问、下标访问可作为赋值目标
- 自动分号插入（ASI）的简化处理：`optional_semicolon` 配合换行检测

### 3.3 运行时值模型（`src/value.rs`）

- 基础类型：`Number(f64)`、`String(String)`、`Bool(bool)`、`Null`、`Undefined`、`Object(ObjectRef)`
- 对象模型：`Rc<RefCell<Object>>` 实现共享可变引用，解决 Rust 所有权与 JS 引用语义的冲突
- 对象内部类型（`Internal`）：`Plain`、`Array(Vec<Option<Value>>)`、`Function { params, body }`、`Native(&'static str)`、`Bound { target, bound_this, bound_args }`
- 原型链支持：`Object.proto` 字段，`Object::lookup` 递归查找
- 抽象相等比较（`abstract_eq`）：实现 ECMAScript 抽象相等算法
- `SameValueZero` 比较：用于 `Array.prototype.includes` 的 NaN 相等判断

### 3.4 解释器（`src/interpreter.rs`）

- **词法环境**：`Env` 结构体实现链式作用域，支持 `define`、`get`、`assign`，闭包通过 `closures: HashMap<usize, Rc<RefCell<Env>>>` 保存
- **控制流**：`Flow` 枚举（`Value`、`Return`、`Throw`、`Break`、`Continue`）统一处理语句执行结果
- **函数调用**：`call` 方法处理普通调用和 `new` 构造调用，构造时创建新对象并设置原型
- **方法调用**：`eval_callee` 通过成员访问表达式提取 `this` 绑定
- **内置函数注册**：`install_builtins` 方法注册所有标准库函数，包括 `Object`、`Array`、`String`、`Number`、`Boolean`、`Error` 系列构造函数及其原型方法
- **资源限制**：`step` 方法计数执行步数，`enter_call`/`leave_call` 跟踪调用深度，`push_output` 限制输出行数

### 3.5 Agent 接口（`src/agent.rs`）

- `AgentRuntime`：封装执行上下文，提供 `run` 方法
- `RuntimeLimits`：统一资源限制配置
- `AgentToolResult`：结构化结果，支持 JSON 序列化
- `AgentErrorKind`：稳定错误分类（`lex`、`parse`、`runtime`、`source_limit`、`step_limit`、`call_depth_limit`）

### 3.6 标准库实现

已实现的 built-in 包括：

- **Object 系列**：`Object`、`Object.create`、`Object.defineProperty`、`Object.getOwnPropertyDescriptor`、`Object.getPrototypeOf`、`Object.keys`、`Object.values`、`Object.prototype.toString`、`Object.prototype.hasOwnProperty`、`Object.prototype.propertyIsEnumerable`
- **Array 系列**：`Array`、`Array.isArray`、`Array.from`、`Array.prototype.map`、`filter`、`forEach`、`indexOf`、`includes`、`slice`、`join`、`push`、`pop`、`shift`、`unshift`、`splice`、`sort`、`reverse`、`concat`、`reduce`、`reduceRight`、`some`、`every`、`find`、`findIndex`、`fill`、`flat`、`lastIndexOf`
- **String 系列**：`String`、`String.fromCharCode`、`String.prototype.slice`、`substring`、`indexOf`、`lastIndexOf`、`charAt`、`charCodeAt`、`trim`、`trimStart`、`trimEnd`、`toLowerCase`、`toUpperCase`、`concat`、`replace`、`split`、`startsWith`、`endsWith`、`includes`、`repeat`、`padStart`、`padEnd`
- **其他**：`Number`、`Boolean`、`Error`/`TypeError`/`SyntaxError`/`ReferenceError`/`RangeError`、`isNaN`、`JSON.stringify`（占位实现）、`Function.prototype.call`/`apply`/`bind`、`print`

---

## 四、开发过程中遇到的问题和解决方法

### 4.1 Rust 所有权与 JS 引用语义的冲突

**问题**：JavaScript 对象是引用类型，多个变量可以引用同一个对象。Rust 的所有权模型不允许共享可变引用。

**解决方法**：采用 `Rc<RefCell<Object>>`（`ObjectRef`）模式。`Rc` 允许共享所有权，`RefCell` 提供运行时借用检查。这一模式贯穿整个对象模型，包括原型链、数组元素、函数闭包等。

### 4.2 闭包环境捕获

**问题**：JS 闭包需要捕获定义时的词法环境，但解释器在函数定义时尚未执行，环境可能在后续执行中变化。

**解决方法**：在 `make_function` 时调用 `remember_closure`，将当前环境的 `Rc` 指针存入 `closures: HashMap<usize, Rc<RefCell<Env>>>`，以函数对象的指针地址作为键。函数调用时从闭包表中恢复环境。

### 4.3 数组空洞（hole）与 undefined 的区分

**问题**：JS 中 `let a = []; a.length = 3;` 创建了三个空洞，`a[0]` 返回 `undefined`，但 `a.hasOwnProperty('0')` 返回 `false`。单纯用 `Option<Value>` 无法区分"空洞"和"值为 undefined 的元素"。

**解决方法**：数组内部使用 `Vec<Option<Value>>`，其中 `None` 表示空洞，`Some(Value::Undefined)` 表示显式赋值为 undefined。在读取时，`None` 返回 `Value::Undefined`（模拟 JS 行为），但在 `hasOwnProperty`、`propertyIsEnumerable` 等检查中区分二者。

### 4.4 Object.defineProperty 自引用描述符

**问题**：`Object.defineProperty(o, 'x', o)` 中，描述符对象和目标对象是同一个引用。在读取描述符属性时，如果先写入目标再读取，会导致读取到刚写入的值。

**解决方法**：在 `call_native` 的 `"Object.defineProperty"` 分支中，先完整读取描述符的 `value`、`writable`、`enumerable` 等字段到局部变量，再执行属性写入操作，避免别名冲突。

### 4.5 Array.prototype.forEach 的实时迭代

**问题**：`forEach` 的回调可能会修改源数组（添加、删除元素或修改长度），且 `forEach` 应当读取实时状态。

**解决方法**：`forEach` 实现中，先记录初始长度，然后在每次迭代时从源数组读取当前值。如果数组被截断，后续迭代自然停止。使用 `RefCell` 的运行时借用检查确保在迭代过程中允许修改。

### 4.6 自动分号插入（ASI）的简化处理

**问题**：JS 的自动分号插入规则复杂，涉及语句结束判断、换行检测等。

**解决方法**：实现了简化版 ASI，通过 `optional_semicolon` 方法在语句末尾可选消费分号。对于未初始化的 `let` 声明，使用 `at_statement_end_after` 检查后续 token 是否在下一行，若是则允许省略初始化表达式。

### 4.7 字符串原型方法的自动装箱

**问题**：JS 中 `"hello".toUpperCase()` 在字符串字面量上调用方法，需要临时将原始类型包装为对象。

**解决方法**：在 `get_property_on_value` 中，对 `Value::String`、`Value::Number`、`Value::Bool` 分别查找对应的原型对象（`string_proto`、`number_proto`、`boolean_proto`），实现自动装箱语义。

### 4.8 调用深度限制的递归计数

**问题**：递归调用需要准确计数调用深度，并在超出限制时立即返回错误，同时正确恢复调用深度。

**解决方法**：`enter_call` 在调用前自增深度并检查限制，`leave_call` 在调用结束后自减深度。如果超出限制，`enter_call` 先调用 `leave_call` 恢复计数再返回错误，确保深度计数器始终平衡。

---

## 五、非本队来源说明

### 5.1 代码来源

本项目所有代码均为原创，无外部代码引入。具体说明如下：

- **Cargo.toml 依赖为空**：项目不依赖任何第三方 Rust crate，仅使用 Rust 标准库
- **非套壳声明**：项目不是 QuickJS、Boa、V8、Node.js 或 Deno 的包装/封装
- **词法分析器**：手写实现，基于字符级扫描
- **语法分析器**：手写递归下降解析器
- **AST 数据结构**：自研设计
- **运行时值模型**：自研实现
- **解释器**：树遍历解释器，自研实现
- **Agent 接口**：自研设计

### 5.2 参考标准

- **ECMAScript 语言规范**（ECMA-262）：作为 JavaScript 语义的参考标准，但并未复制任何规范代码
- **JetStream 基准测试**：README 中提及 JetStream 作为未来性能测试方向的参考（概念性参考，非代码）

### 5.3 文档翻译

- `README-zh-cn.md`、`docs/agent-tool-zh-cn.md`、`docs/language-subset-zh-cn.md` 是英文文档的中文翻译版本

---

## 六、AI 工具及大模型使用场景

### 6.1 AI 辅助开发工具

| 工具/模型 | 使用场景 |
|-----------|----------|
| OpenCode (Claude Code) | 代码生成、代码审查、调试辅助、文档编写、测试编写 |
| Claude（Anthropic） | 架构设计讨论、问题排查、代码优化建议 |

### 6.2 使用说明

- AI 工具主要用于辅助编码、调试和文档编写
- 所有代码均由团队成员审查和修改，AI 生成的代码仅作为起点
- 项目架构设计和关键决策由团队主导，AI 提供建议和参考
- 测试用例覆盖了核心功能，确保代码质量

---

## 七、开发过程总结

### 7.1 开发阶段

1. **初始化阶段**（~5 个 commits）：搭建 Rust 项目脚手架，实现基础 lexer 和 parser
2. **核心引擎阶段**（~15 个 commits）：完成解释器核心，支持变量、函数、控制流、闭包
3. **标准库扩展阶段**（~40 个 commits）：逐步添加 Object、Array、String 等标准库方法
4. **Agent 接口阶段**（~10 个 commits）：实现 `--agent-eval` 模式、资源限制、结构化输出
5. **完善与文档阶段**（~15 个 commits）：补充测试、完善文档、修复边界情况

### 7.2 代码规模

- 源文件：10 个 Rust 模块（不含测试约 3000 行）
- 测试：各模块内置单元测试 + 集成测试
- 文档：README 中英文 + 3 份技术文档中英文

### 7.3 经验教训

1. **从简单开始**：MVP 阶段优先实现核心语法，再逐步扩展标准库
2. **测试先行**：每个功能模块都附带单元测试，确保回归安全
3. **渐进式复杂度**：先实现树遍历解释器，后续再考虑字节码编译优化
4. **Agent 场景驱动**：功能优先级由 Agent 常用脚本场景决定，而非追求完整 ECMAScript 兼容