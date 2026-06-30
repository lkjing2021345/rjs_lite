# rjs_lite 过程性文档

## 一、目标描述

### 1.1 项目目标

本项目参加"面向 AI Agent 的轻量级执行引擎"赛题，目标是构建一个**轻量、可嵌入、零外部依赖**的 JavaScript 执行 Runtime，专为 AI Agent 工作流中的短生命周期、高频本地工具调用场景设计。

### 1.2 核心目标

1. 建立原生 JS 引擎核心架构（lexer → parser → AST → interpreter）
2. 支持基础 JS 语法执行，能通过自带单元测试和 CLI 测试
3. 提供 Agent 可直接消费的结构化 JSON 执行结果
4. 为 test262 接入、标准库和性能优化预留清晰演进路线

### 1.3 成功标准

- 不依赖任何第三方 JS 引擎（非套壳）
- 不依赖任何第三方 Rust crate
- 通过自有单元测试覆盖核心功能
- Agent 工具模式输出稳定 JSON 结果

---

## 二、比赛题目分析和相关资料调研

### 2.1 赛题解读

赛题要求实现一个"面向 AI Agent 的轻量级执行引擎"。核心需求分析：

- **轻量级**：启动快、占用小、无外部依赖
- **Agent 友好**：输出结构化结果，便于 Agent 解析和处理
- **可嵌入**：能作为库或 CLI 工具被程序调用
- **安全可控**：具备资源限制机制，防止失控脚本

### 2.2 技术选型调研

| 方案 | 优势 | 劣势 | 结论 |
|------|------|------|------|
| 套壳 V8/QuickJS | 兼容性好 | 包体大、依赖重、不符合赛题要求 | 不采用 |
| 套壳 Node.js | 功能完整 | 启动慢、依赖重 | 不采用 |
| 自研 Rust 引擎 | 轻量、零依赖、可控 | 开发工作量大、兼容性有限 | **采用** |

### 2.3 参考资料

- **ECMAScript 语言规范**（ECMA-262）：作为 JavaScript 语义的参考标准
- **test262 测试套件**：ECMAScript 官方一致性测试集，用于验证引擎兼容性
- **Rust 标准库文档**：实现零外部依赖的技术基础
- **JetStream 基准测试**：作为未来性能测试方向的参考

---

## 三、系统框架设计

### 3.1 总体架构

执行流水线：

```
JS 源码 -> lexer -> tokens -> parser -> AST -> interpreter -> Value
```

采用经典编译前端四阶段设计，不生成字节码或机器码，直接由树遍历解释器执行 AST。

### 3.2 模块划分

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
| 库入口 | `src/lib.rs` | 公开 API 和便利函数 |

### 3.3 关键设计决策

1. **零外部依赖**：不使用任何第三方 Rust crate，完全依靠 Rust 标准库
2. **手写实现而非套壳**：自研 lexer、parser、AST、解释器，不包装 QuickJS、Boa、V8、Node.js 或 Deno
3. **树遍历解释器**：MVP 阶段优先保证正确性和可扩展性
4. **Agent 优先接口**：`--agent-eval` 模式输出结构化 JSON
5. **资源限制机制**：源码大小 64KB / 步数 100K / 深度 16 层 / 输出 256 行

### 3.4 运行时值模型

```
Value = Number(f64) | String(String) | Bool(bool) | Null | Undefined | Object(ObjectRef)
Object = { props: HashMap, proto: Option<ObjectRef>, internal: Internal }
Internal = Plain | Array(Vec<Option<Value>>) | Function{params,body} | Native(name) | Bound{target,this,args}
```

采用 `Rc<RefCell<Object>>` 实现共享可变引用，解决 Rust 所有权与 JS 引用语义的冲突。

---

## 四、开发计划

### 4.1 总体计划

项目开发周期为 2026 年 6 月 5 日至 6 月 30 日，分为 5 个阶段：

### 4.2 阶段计划与完成情况

| 阶段 | 时间 | 计划任务 | 完成情况 |
|------|------|----------|----------|
| 第一阶段：基础框架搭建 | 6/5 – 6/9 | Rust 脚手架、lexer、parser、AST、test262 框架 | 全部完成 |
| 第二阶段：核心引擎开发 | 6/10 – 6/13 | 解释器、闭包、原型链、控制流、函数 | 全部完成 |
| 第三阶段：Agent 接口 & REPL | 6/15 – 6/17 | REPL、--agent-eval、资源限制、错误分类 | 全部完成 |
| 第四阶段：标准库扩展 | 6/23 – 6/30 | Object/Array/String/Math/Error 等 80+ 函数 | 全部完成 |
| 第五阶段：高级语法补充 | 6/30 | 模板字面量、箭头函数、位运算、for...in | 全部完成 |

### 4.3 任务分解

合计 87+ 次提交，覆盖：
- 词法分析器：字面量、运算符、注释、错误位置
- 语法分析器：21 种表达式、14 种语句、优先级处理
- 解释器：词法作用域、闭包、原型链、this 绑定、new 构造
- 标准库：Object 8 方法、Array 27 方法、String 18 方法、Math 全部、Error 5 类型
- Agent 模式：结构化 JSON、6 种错误分类、4 维资源限制
- 高级语法：模板字面量、箭头函数、位运算符、for...in、delete/void/in

---

## 五、比赛过程中的重要进展

### 5.1 初始阶段（6/5）

- 初始化 Rust 项目脚手架
- 完成中英文 README 文档
- 实现基础 Agent 工具接口原型

### 5.2 语法引擎突破（6/7 – 6/9）

- 完成 typeof 运算符、未初始化 let 声明、break/continue
- 支持三元表达式、复合赋值、自增自减、下标访问
- 完成 REPL 交互式解释器
- 添加 test262 测试框架
- 完成多变量声明、函数表达式、成员访问/赋值、对象/数组基础模型、throw、循环、instanceof、typeof

### 5.3 引擎重写完成（6/12）

- 完成了解释器重写，支持闭包、原型链、this 绑定
- 实现了完整的词法作用域链式环境

### 5.4 测试与完善（6/13 – 6/17）

- 完善了测试机功能
- 增加 Agent 执行步数限制、调用深度限制
- 输出限制防止内存溢出
- 完成 REPL 功能更新

### 5.5 标准库大规模扩展（6/23 – 6/30）

- Object 系列：keys、values、create、defineProperty、getOwnPropertyDescriptor、hasOwnProperty、propertyIsEnumerable
- Array 系列：isArray、indexOf、includes、slice、push、pop、shift、forEach、map、filter、join、sort、reverse、concat、reduce、reduceRight、some、every、find、findIndex、fill、flat、lastIndexOf、unshift、splice、from
- String 系列：slice、substring、indexOf、lastIndexOf、charAt、charCodeAt、trim、trimStart、trimEnd、toLowerCase、toUpperCase、concat、replace、split、startsWith、endsWith、includes、repeat、padStart、padEnd、fromCharCode
- 其他：Function.prototype.call/apply/bind、Math 对象、JSON.stringify、Error 系列、Infinity/NaN/undefined/parseInt/parseFloat/isFinite/eval/Function 构造函数

### 5.6 高级语法补充（6/30）

- 模板字面量（backtick strings）
- 箭头函数（=>）
- 位运算符（& | ^ ~ << >> >>>）
- for...in 循环
- delete / void / in 运算符

---

## 六、系统测试情况

### 6.1 单元测试

项目包含 71 个单元测试，全部通过。

| 测试模块 | 位置 | 覆盖内容 |
|----------|------|----------|
| lexer 测试 | `src/lexer.rs` | 注释解析、非法字符串检测 |
| parser 测试 | `src/parser.rs` | 表达式优先级、函数解析、未初始化 let、const 初始化检查 |
| interpreter 测试 | `src/interpreter.rs` | 60+ 测试覆盖所有核心功能 |
| agent 测试 | `src/agent.rs` | 结构化结果、错误分类、资源限制、JSON 序列化 |
| lib 测试 | `src/lib.rs` | 端到端执行、运算符、函数、循环、typeof、资源限制 |
| CLI 集成测试 | `tests/cli.rs` | CLI 执行、参数解析 |

### 6.2 测试覆盖范围

- 算术运算与变量
- 函数定义与调用（含闭包）
- 控制流：if/else、while、for、switch、break、continue
- 异常处理：throw/try/catch/finally
- 对象与数组：字面量、成员访问、原型链
- 运算符：typeof、instanceof、抽象相等、严格相等
- 资源限制：步数限制、调用深度限制、输出限制
- 标准库：Object/Array/String 各方法
- Agent 模式：JSON 输出、错误分类、source_limit

### 6.3 test262 测试

项目提供了 `run_test262.py` 测试运行器，需要外部 test262 checkout 配合使用。当前引擎尚未运行完整的 test262 测试套件，后续计划推进 test262 子集覆盖。

---

## 七、遇到的主要问题和解决方法

### 7.1 Rust 所有权与 JS 引用语义的冲突

**问题**：JavaScript 对象是引用类型，多个变量可以引用同一个对象。Rust 的所有权模型不允许共享可变引用。

**解决方法**：采用 `Rc<RefCell<Object>>`（`ObjectRef`）模式。`Rc` 允许共享所有权，`RefCell` 提供运行时借用检查。该模式贯穿整个对象模型，包括原型链、数组元素、函数闭包等。

### 7.2 闭包环境捕获

**问题**：JS 闭包需要捕获定义时的词法环境，但环境可能在后续执行中变化。

**解决方法**：在 `make_function` 时调用 `remember_closure`，将当前环境的 `Rc` 指针存入 `closures: HashMap<usize, Rc<RefCell<Env>>>`，以函数对象的指针地址作为键。

### 7.3 数组空洞（hole）与 undefined 的区分

**问题**：JS 中 `let a = []; a.length = 3` 创建了三个空洞，`a[0]` 返回 `undefined`，但 `a.hasOwnProperty('0')` 返回 `false`。单纯用 `Option<Value>` 无法区分。

**解决方法**：数组内部使用 `Vec<Option<Value>>`，`None` 表示空洞，`Some(Value::Undefined)` 表示显式赋值。

### 7.4 Object.defineProperty 自引用描述符

**问题**：`Object.defineProperty(o, 'x', o)` 中，描述符和目标对象是同一引用，读写互相干扰。

**解决方法**：先完整读取描述符的 `value`、`writable`、`enumerable` 到局部变量，再执行写入。

### 7.5 Array.prototype.forEach 的实时迭代

**问题**：`forEach` 回调可能修改源数组，需读取实时状态。

**解决方法**：先记录初始长度，每次迭代时从源数组读取当前值，被截断时自然停止。

### 7.6 自动分号插入（ASI）的简化处理

**问题**：JS 的 ASI 规则复杂，涉及语句结束和换行判断。

**解决方法**：`optional_semicolon` 可选消费分号，`at_statement_end_after` 结合换行检测判断语句结束。

### 7.7 字符串原型方法的自动装箱

**问题**：JS 中 `"hello".toUpperCase()` 在原始类型上调用方法需临时装箱。

**解决方法**：`get_property_on_value` 中对 `String`/`Number`/`Bool` 分别查找对应原型对象。

### 7.8 调用深度限制的递归计数

**问题**：递归调用需准确计数深度，超出限制时正确恢复。

**解决方法**：`enter_call` 自增并检查，`leave_call` 自减。超限时先恢复再返回错误。

---

## 八、分工和协作

### 8.1 团队成员

| 姓名 | 角色 | 主要职责 |
|------|------|----------|
| 何禹颃 | 队长 / 核心架构师 | 项目架构设计、lexer/parser/解释器核心引擎、Agent 接口、代码审查 |
| 真笑君 | 队员 / 标准库开发 | Object/Array/String 标准库、错误类型系统、单元测试、边界情况处理 |
| 李春雨 | 队员 / 功能扩展 & 工程化 | REPL、CLI、高级语法、Math 对象、test262 框架、文档编写 |

### 8.2 协作方式

- **版本管理**：使用 Git 进行版本控制，采用 feature branch 工作流
- **代码审查**：队长负责代码审查，合并前确认测试通过
- **沟通方式**：团队内部即时通讯 + 面对面讨论
- **任务分配**：基于个人技术特长分工，交叉协作解决难点

### 8.3 开发流程

1. 从 `test/all-remote-branches` 创建功能分支
2. 实现功能并编写单元测试
3. 运行 `cargo test` 确保测试通过
4. 提交 Pull Request 并代码审查
5. 合并回 `test/all-remote-branches`

---

## 九、提交仓库目录和文件描述

### 9.1 目录结构

```
rjs_lite/
├── Cargo.toml              # Rust 项目配置（零外部依赖）
├── Cargo.lock              # 依赖锁定文件
├── README.md               # 英文 README
├── README-zh-cn.md         # 中文 README
├── run_test262.py          # test262 测试运行器
├── src/
│   ├── main.rs             # CLI 入口（REPL/文件执行/Agent 模式）
│   ├── lib.rs              # 库入口与公开 API
│   ├── lexer.rs            # 词法分析器
│   ├── parser.rs           # 递归下降语法分析器
│   ├── ast.rs              # AST 数据结构
│   ├── token.rs            # Token 类型与关键字映射
│   ├── value.rs            # 运行时值模型
│   ├── interpreter.rs      # 树遍历解释器
│   ├── agent.rs            # Agent 工具接口
│   └── error.rs            # 错误类型与位置信息
├── tests/
│   └── cli.rs              # CLI 集成测试
├── docs/
│   ├── agent-tool.md       # Agent 工具接口文档（英文）
│   ├── agent-tool-zh-cn.md # Agent 工具接口文档（中文）
│   ├── language-subset.md  # 语言子集文档（英文）
│   ├── language-subset-zh-cn.md # 语言子集文档（中文）
│   └── process/
│       ├── process-documentation.md      # 过程性文档（Markdown）
│       └── process-documentation.docx    # 过程性文档（Word）
├── examples/
│   └── demo.js             # 示例 JS 文件
└── .gitignore              # Git 忽略规则
```

### 9.2 核心文件说明

| 文件 | 行数 | 说明 |
|------|------|------|
| `src/lexer.rs` | 302 | 字符级词法分析器，生成 Token 序列 |
| `src/parser.rs` | 748 | 递归下降语法分析器，14 种语句 + 21 种表达式 |
| `src/ast.rs` | 142 | AST 数据结构定义 |
| `src/value.rs` | 274 | 运行时值模型，5 种基础类型 + 对象系统 |
| `src/interpreter.rs` | 3201 | 树遍历解释器，含词法环境、闭包、80+ 内置函数 |
| `src/agent.rs` | 365 | Agent 工具接口，含资源限制和结构化 JSON |
| `src/main.rs` | 172 | CLI 入口，REPL/文件执行/Agent 模式 |
| `src/lib.rs` | 232 | 库入口，公开 API |
| `src/error.rs` | 132 | 错误类型系统 |
| `src/token.rs` | 114 | Token 类型定义和关键字映射 |

---

## 十、比赛收获

### 10.1 技术收获

1. **深入理解 JS 引擎原理**：通过手写实现 lexer、parser、AST 和解释器，深入理解了 JavaScript 语言规范（ECMA-262）中的词法作用域、闭包、原型链、this 绑定等核心机制。

2. **Rust 系统编程实践**：大量使用 `Rc<RefCell<>>` 模式解决所有权与引用语义的冲突，深入理解了 Rust 的内存安全和借用检查机制。

3. **编译原理工程实践**：实现了完整的词法分析、语法分析（递归下降）、AST 遍历解释执行流水线，加深了对编译原理的理解。

4. **标准库设计经验**：通过实现 80+ 内置函数，积累了 Object/Array/String 标准库的 API 设计经验。

### 10.2 团队协作收获

1. **版本控制流程**：实践了 feature branch 工作流、代码审查、合并策略
2. **分工协作**：基于个人技术特长合理分工，提高开发效率
3. **沟通效率**：通过及时沟通解决技术难题，避免重复劳动

### 10.3 工程经验

1. **从简单开始**：MVP 阶段优先实现核心语法，再逐步扩展标准库
2. **测试先行**：每个功能模块都附带单元测试，确保回归安全
3. **渐进式复杂度**：先实现树遍历解释器，后续再考虑字节码编译优化
4. **Agent 场景驱动**：功能优先级由 Agent 常用脚本场景决定

### 10.4 项目成果总结

- 87+ 次 Git 提交，10 个核心 Rust 模块
- 71 个单元测试全部通过
- 80+ 个标准库内置函数
- 零外部依赖，纯 Rust 标准库实现
- 完整的中英文技术文档
- Agent 工具模式支持结构化 JSON 输出

---

*文档编写日期：2026 年 6 月 30 日*