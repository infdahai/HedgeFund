# Opinion HFT - Formatting Standards

**Version**: 1.1.0
**Effective Date**: 2026-01-20
**Status**: Active

---

## 目标 (Goal)

建立单一、确定的格式化标准，消除 rustfmt 和 clippy 规则之间的混淆。

---

## 核心原则 (Core Principles)

1. **唯一性 (Single Source of Truth)**: 项目中只有一个格式化标准
2. **自动化 (Automation)**: 使用 rustfmt 自动格式化，依赖人工记忆
3. **可执行 (Enforceable)**: 规则可以通过工具检查和强制执行
4. **最小化 (Minimalism)**: 规则简单明确，减少认知负担

---

## 1. rustfmt 配置 (Mechanical Formatting)

### 配置文件: `rustfmt.toml`

```toml
edition = "2021"
hard_tabs = false
tab_spaces = 4
max_width = 100
```

### 覆盖范围

- ✅ **缩进**: 4 空格
- ✅ **行宽**: 100 字符（默认值）
- ⚠️ **不覆盖**: 链式调用、字符串字面量等（保持默认）

### 理由

- Rust 官方标准
- 最大化可读性
- 与现有代码库风格一致

---

## 2. clippy 配置 (Linting and Style)

### 项目特有规则

不同于 rustfmt 默认值，以下规则必须遵守：

#### 2.1. 错误处理 (Error Handling)

- ✅ **优先使用结构化错误类型**: `crate::error::Error`
- ✅ **使用 `.into()` 转换错误消息**: `.into()` 而非 `format!()`（保留类型信息）
- ❌ **禁止**: 字符串格式化错误（丢失类型信息）

**示例**:
```rust
// ✅ 正确
return Err(Error::ApiError {
    status: status.as_u16(),
    message: format!("HTTP error {}", status),
});

// ❌ 错误
return Err(Error::ApiError(
    format!("HTTP error {}", status).into()
));
```

#### 2.2. 命名规范 (Naming)

- ✅ **类型**: `PascalCase`（首字母大写）
- ✅ **函数/方法**: `snake_case`（下划线分隔）
- ✅ **常量**: `SCREAMING_SNAKE_CASE`（全大写下划线）

**例外**: 外部依赖的命名遵循其风格

#### 2.3. 结构体 (Structs)

- ✅ **字段顺序**: 公共 API 字段在前，私有字段在后
- ✅ **derive 宏**: 使用 `#[derive(Debug, Clone)]`（小类型可加 `#[derive(Copy)]`）

**例外**: 测试辅助结构体可按逻辑顺序排列

#### 2.4. 注释 (Comments)

- ✅ **公共 API**: 使用 `///` 三斜线文档注释
- ⚠️ **内部实现**: 优先使用 `//` 双斜线，保持简洁
- ❌ **禁止**: 块注释 `/* */`（除非在临时代码中）

**注释内容指南**:
- 说明"是什么"（what）
- 避免"怎么做"（how to implement）

**示例**:
```rust
/// Represents a detected arbitrage opportunity.
///
/// This struct contains all information needed to execute
/// an arbitrage trade, including cost estimates and profit margins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    /// Unique identifier for tracking this opportunity
    pub id: String,

    /// Type of arbitrage strategy detected
    pub arb_type: ArbitrageType,

    // Internal implementation details
    // Timestamp when opportunity was detected
    pub detected_at: u64,
}
```

#### 2.5. 异步代码 (Async/Await)

- ✅ **spawn 后处理**: 使用 `.await`（保留 JoinHandle 或取消机制）
- ✅ **select!**: 多路事件选择
- ❌ **禁止**: 阻塞调用（`std::thread::sleep`）

**示例**:
```rust
// ✅ 正确
let handle = tokio::spawn(async move { /* ... */ });

// ✅ 正确
tokio::select! {
    Some(event) = rx.recv() => { /* ... */ },
    _ = tick.tick() => { /* ... */ },
}
```

#### 2.6. 测试代码 (Test Code)

- ✅ **测试函数**: 使用 `#[tokio::test]`
- ✅ **测试名称**: `test_<场景>_<行为>`（如 `test_fetch_active_markets`）
- ⚠️ **集成测试**: 需要真实 `.env` 配置，以 `live_` 开头

**测试数据常量**:
- 最小订单金额: `1.30` USDT
- 最小手续费: `0.50` USDT
- 合理价格范围: `0.3 - 0.7`

---

## 3. 与 rustfmt/clippy 的差异处理

### 3.1. rustfmt 覆盖但可接受的差异

以下情况下允许与 rustfmt 默认格式不同：

- **长 URL/路径字符串**: 可超出 100 字符限制
- **复杂错误消息**: `format!()` 宏导致的长行

### 3.2. clippy 警告处理策略

- **立即修复**: 阻止编译的警告必须先修复
- **技术债务**: 非阻塞性警告可记录技术债务，允许后续处理
- **误报**: 如确认是误报，可添加 `#[allow(...)]`

---

## 4. 执行工作流 (Execution Workflow)

### 开发阶段

1. **代码修改**
   ```bash
   # 编辑代码后
   cargo fmt --all
   ```

2. **检查**
   ```bash
   # 检查格式化（可选）
   cargo fmt --all --check

   # 检查 Lint 警告（必须 0 warnings）
   cargo clippy --all -D warnings
   ```

3. **预提交检查**
   - [ ] `cargo fmt` 通过（无格式变更）
   - [ ] `cargo clippy` 无警告（0 warnings）
   - [ ] 所有公共 API 有文档注释
   - [ ] 没有不安全的代码（除非有文档说明）

### CI/CD 集成

**GitHub Actions / 自动化工具**:
- 添加预提交 hook 自动执行 `cargo fmt`
- 在 CI 中运行 `cargo clippy --all -D warnings` 并阻止非零退出

---

## 5. 常见问题 (FAQ)

### Q1: 为什么 rustfmt 和 clippy 规则冲突？

**A**: 它们关注不同方面：
- **rustfmt**: 机械格式化（缩进、空格、换行）
- **clippy**: 代码质量和最佳实践（正确性、性能、复杂度）

**结论**: 两者互补，不冲突。

### Q2: 如何处理不符合规则的旧代码？

**A**:
1. 运行 `cargo fmt --all` 批量格式化
2. 单独提交格式化变更
3. 后续逐步修复 clippy 警告

### Q3: 测试代码可以不遵循规则吗？

**A**: 测试辅助结构可简化规则，但核心测试逻辑必须遵循命名和错误处理规范。

---

## 版本历史 (Changelog)

| 版本 | 日期 | 变更内容 |
|-------|------|----------|
| 1.0 | 2026-01-20 | 初始版本，建立统一格式化标准 |

---

## 参考资料 (References)

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust Style Guide](https://rust-lang.github.io/style-guide/)
- [clippy Lints Documentation](https://rust-lang.github.io/rust-clippy/)
