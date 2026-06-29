# 支持的 JavaScript 子集

本文档描述 `rjs_lite` 当前面向的 JavaScript 子集。项目目标是服务短生命周期
AI Agent 脚本的轻量级 Runtime，而不是完整 ECMAScript 引擎。

## 已支持语法

- 字面量：数字、字符串、布尔值、`null` 和 `undefined`
- 绑定：`let`、未初始化 `let`、`var` 和 `const`
- 表达式：算术、比较、相等、逻辑、一元、三元、`typeof` 和 `instanceof`
- 赋值：可变绑定赋值、成员赋值、下标赋值、复合赋值、前缀更新和后缀更新
- 控制流：代码块、`if`、`else`、`while`、`for`、`switch`、`break` 和
  `continue`
- 函数：函数声明、函数表达式、函数调用、闭包、`this`、`new` 和 `return`
- 异常：`throw`、`try`、`catch` 和 `finally`
- 数据结构：对象字面量、数组字面量、成员访问、下标访问、数组 `length`
  和原型查找

## 已支持运行时表面

- 最小宿主函数：`print(value)`
- 基础构造器：`Object`、`Array`、`String`、`Number`、`Boolean` 和常见错误构造器
- 基础工具：`isNaN`
- 对象工具：`Object.defineProperty`、`Object.getOwnPropertyDescriptor`、
  `Object.keys` 和 `Object.values`
- 占位 JSON 支持：`JSON.stringify`
- 小型原型方法面：`Object.prototype.toString`、
  `Object.prototype.hasOwnProperty`、`Array.prototype.join` 和
  `Array.prototype.map`、`Array.prototype.filter`、`Array.prototype.forEach`、
  `Array.prototype.push`
- `Array.prototype.map`、`Array.prototype.filter` 和
  `Array.prototype.forEach` 回调会接收当前值、从 0 开始的索引和源数组
- 交互式 REPL，支持状态保留、多行输入和点命令

## Agent 工具契约

Agent 模式返回一个 JSON 对象，包含：

- `ok`
- `request_id`
- `value`
- `value_type`
- `output`
- `output_truncated`
- `error_kind`
- `error`

JavaScript 解析失败和运行时失败会编码为 `ok:false` 的普通 JSON 结果，Agent
不需要通过进程失败来判断脚本错误。

当前 `error_kind` 取值包括：

- `lex`
- `parse`
- `runtime`
- `source_limit`
- `step_limit`
- `call_depth_limit`

Agent 模式默认应用这些运行限制：

- 源码文本：64 KiB
- 捕获输出：256 行
- 解释器执行步数：100,000
- 嵌套用户函数调用：16 层

## 部分支持语义

以下能力已经存在，但仍是不完整实现：

- 对象、数组、构造器、原型和属性描述符语义
- 数组 holes 已经和显式 `undefined` 区分表示，但完整稀疏数组行为仍不完整
- JavaScript 隐式类型转换规则
- 标准库行为
- Error 对象行为和带类型异常匹配
- `JSON.stringify`，当前仍是最小占位实现
- REPL 行编辑和历史记录

## 暂不支持

- class
- module
- async 函数和 promise
- generator
- regexp
- symbol
- BigInt
- 完整严格模式行为
- 浏览器 API、DOM API、npm packages、文件 API、网络 API、进程 API 和 OS API
- JIT、字节码 VM 或高级优化流水线

## test262 方向

`run_test262.py` 是可选的本地探索 runner，需要仓库根目录存在上游 `test262/`
检出目录。它应该用于子集跟踪，而不是用于声明完整 ECMAScript 兼容性。

后续 test262 工作应维护子集清单，记录通过/失败分类，并清晰区分已支持语法的失败
和未支持特性的失败。
