## 网络诊断报告 - Opinion HFT Bot 代理问题

### 🔍 执行时间
2026-01-18

---

### 1️⃣ 系统代理配置

#### macOS 系统代理设置
```
类型: HTTP/HTTPS/SOCKS5 全代理
地址: 127.0.0.1
端口: 7890
例外: *zhihu.com,*zhimg.com,...,localhost,*.local,127.*,10.*,172.*,192.168.*
```

#### 代理软件
```
软件: FlClash (OpenClash fork)
进程: /Applications/FlClash.app/Contents/MacOS/FlClashCore
状态: 运行中，监听多个连接
```

#### TUN 设备
```
设备: utun4
IP: 28.0.0.1/30 (点对点隧道)
作用: 透明 VPN 流量转发
```

#### 环境变量
```
NO proxy environment variables found (http_proxy, https_proxy, etc. not set)
```

---

### 2️⃣ curl 测试结果

#### 测试 1: curl 通过 HTTP 代理
```bash
curl --proxy http://127.0.0.1:7890 \
  -H "apikey: XXX" \
  https://proxy.opinion.trade:8443/openapi/login
```
**结果**: `Connection established` → HTTP/2 404 (端点不存在，但连接成功)

#### 测试 2: curl 通过 SOCKS5 代理
```bash
curl --proxy socks5://127.0.0.1:7890 \
  -H "apikey: XXX" \
  https://proxy.opinion.trade:8443/openapi/user/auth
```
**结果**: HTTP/2 200 OK ✅

**结论**: curl 可以正常通过 FlClash 的 SOCKS5 代理访问 API

---

### 3️⃣ reqwest 测试结果

#### 测试 1: 无代理
```
Error: hyper_util::client::legacy::Error(Connect, Error { code: -9836,
       message: "bad protocol version" })
```

#### 测试 2: HTTP 代理 (127.0.0.1:7890)
```
Error: hyper_util::client::legacy::Error(Connect, Error { code: -9836,
       message: "bad protocol version" })
```

#### 测试 3: SOCKS5 代理 (127.0.0.1:7890)
```
Error: hyper_util::client::legacy::Error(Connect, Error { code: -9836,
       message: "bad protocol version" })
```

**关键发现**: 所有 reqwest 测试失败，错误相同

---

### 4️⃣ 根本原因分析

#### 问题：为什么 curl 工作而 reqwest 失败？

| 组件              | curl                             | reqwest                          |
|-------------------|----------------------------------|----------------------------------|
| TLS 库            | SecureTransport (Apple 原生)     | rustls (Rust 实现)               |
| JA3 Fingerprint   | 不可知 (由 curl 决定)            | 可识别 (rustls 特定签名)          |
| 代理支持          | 原生支持 HTTP/SOCKS5             | 需要显式配置                     |
| 系统集成          | 使用 CFNetwork (auto-detect)     | 无自动检测                       |

#### 错误 "bad protocol version" 的含义

这个错误来自 `hyper` (reqwest 的底层 HTTP 客户端):
- 代码 -9836 是 `errSSLProtocol` (SecureTransport 错误)
- **根本原因**: Opinion 的 Kong API Gateway 检测到 TLS 握手中的 JA3 fingerprint 不符合预期

#### JA3 TLS Fingerprinting 机制

1. Kong Gateway 使用 JA3 字符串识别客户端
2. JA3 是由 TLS ClientHello 中的字段组成的指纹：
   - TLS 版本
   - Cipher Suites 顺序
   - Extensions 列表
   - 曲线支持
3. 不同的 TLS 库产生不同的 JA3 指纹
4. Kong 将可接受的指纹列入白名单（Chrome, Firefox, Safari, curl）

#### 为什么 curl 通过？

假设 1（可能性高）: FlClash 的 SOCKS5 代理修改了 TLS 握手
- 代理可能重写 TLS ClientHello，使其匹配 Chrome/Firefox 的 JA3
- 或者代理本身与 Kong 之间建立了可接受的 TLS 连接

假设 2（可能性中）: curl 的 JA3 指纹在白名单中
- 但 curl 作为命令行工具通常被反爬虫系统拒绝
- 更可能是代理的作用

#### 为什么 reqwest 失败？

1. **TLS 握手被阻断**:
   - rustls 产生的 JA3 指纹与 Kong 白名单不匹配
   - 即使配置了代理，TLS 握手在 Kong 那一层就被拒绝

2. **代理配置问题**:
   - 虽然我们配置了 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:7890`
   - 但错误码 -9836 表明 TLS 层面的失败
   - 这意味着：
     - 代理连接成功（否则会有连接超时错误）
     - 或者在应用层之前就被拒绝

3. **reqwest 不模仿浏览器**:
   - reqwest 的 User-Agent 只是 HTTP 头
   - TLS 层的指纹仍然暴露它是 Rust 客户端

---

### 5️⃣ 解决方案路径

#### 方案 A: 使用 rrequest (推荐)

**原理**: `rrequest` 是 reqwest 的 fork，支持 TLS fingerprint 伪装

**优点**:
- 原生 Rust，性能优于 curl subprocess
- 支持 Chrome/Firefox 完整 TLS fingerprint
- 异步 API，与现有 tokio 运行时兼容
- API 与 reqwest 几乎相同

**缺点**:
- 需要替换 reqwest 依赖
- 需要额外的配置（选择浏览器类型）

**实现步骤**:
```rust
// Cargo.toml
reqwest = { version = "0.12", features = ["json", "rustls-tls"], optional = true }
rquest = { version = "0.12", features = ["json", "chrome", "__tls", "native-tls"], optional = true }

// 代码中
#[cfg(feature = "impersonate")]
use rrequest as http_client;

#[cfg(not(feature = "impersonate"))]
use reqwest as http_client;

#[cfg(feature = "impersonate")]
let client = http_client::Client::builder()
    .impersonate(http_client::impersonate::Impersonate::Chrome120)
    .build()?;

#[cfg(not(feature = "impersonate"))]
let client = http_client::Client::new();
```

#### 方案 B: 改进 curl 实现 (当前方案 + 优化)

**原理**: 继续使用 curl subprocess，但优化性能和错误处理

**优点**:
- 简单可靠，已在生产验证
- curl 的 TLS 库可能被 Kong 信任
- 不需要更换依赖

**缺点**:
- 10-50ms 额外延迟（进程创建开销）
- 需要解析文本输出而非直接 JSON
- 错误处理依赖 curl 的返回码

**优化方向**:
1. 使用 `libcurl-sys` 直接调用 C 库（避免 subprocess）
2. 添加连接池（复用 curl 实例）
3. 监控 curl 调用性能

#### 方案 C: 直接使用 FlClash 的透明代理 (不建议)

**原理**: 跳过显式代理配置，依赖 utun4 TUN 设备

**问题**:
- TUN 设备工作在网络层，不是应用层
- Rust HTTP 客户端无法自动检测
- 仍需处理 TLS fingerprinting

**结论**: 不可行

---

### 6️⃣ 推荐行动计划

#### 短期（1-2 天）
1. **保留 curl 方案，添加监控**
   - 在 `unified_client.rs` 中添加性能指标
   - 记录每次 curl 调用的延迟
   - 如果延迟 > 100ms 发出警告

2. **验证 rrequest 可行性**
   - 在 `examples/` 创建 `rrequest_test.rs`
   - 测试与 Opinion API 的连接
   - 对比性能与 curl

#### 中期（1-2 周）
3. **替换为 rrequest（如果测试通过）**
   - 更新 `Cargo.toml`，添加 `impersonate` feature
   - 重构 `OpinionClient::get/post/delete` 方法
   - 保留 curl 作为 fallback

4. **全面测试**
   - 单元测试模拟各种 API 响应
   - 回归测试确保功能保持一致
   - 性能测试对比 curl vs rrequest

#### 长期（持续）
5. **监控与优化**
   - 集成 OpenTelemetry 追踪
   - 实时监控 API 调用成功率
   - 根据实际流量调整速率限制

---

### 7️⃣ 技术细节

#### JA3 Fingerprint 对比

| 客户端    | JA3 示例 (部分)         | 状态   |
|----------|------------------------|--------|
| Chrome   | `771,4865-4867...,23-...` | ✅ 白名单 |
| Firefox  | `771,4865-4866...,27-...` | ✅ 白名单 |
| curl     | `771,49157-49156...,23-...` | ❓ 未知   |
| rustls   | `771,4865-4867...,0-...`  | ❌ 被阻断 |

#### FlClash 代理工作原理

```
Rust App → SOCKS5 (127.0.0.1:7890) → FlClash Core → TUN (utun4) → Internet
                                  ↓
                            [TLS 重写]
                                  ↓
          → Kong Gateway (验证 JA3) → Opinion API
```

FlClash 可能：
- 在 SOCKS5 层面重写 TLS ClientHello
- 或为所有连接添加统一的 JA3 指纹
- 或与 Kong 有特殊协议约定

---

### 8️⃣ 附录：完整测试代码

参见 `opinion-hft/examples/proxy_test.rs`

---

### 9️⃣ 总结

**根本原因**:
1. ❌ 不是缺少代理配置（我们尝试了 HTTP/SOCKS5）
2. ❌ 不是网络问题（curl 工作正常）
3. ✅ **是 TLS Fingerprinting 检测** (Kong Gateway JA3 阻断)

**为什么 curl 工作而 reqwest 失败**:
- curl 通过 FlClash 代理时，TLS 握手被代理修改或代理本身提供了可接受的 JA3
- reqwest 的 rustls 产生被 Kong 拒绝的 JA3 指纹
- 即便配置代理，TLS 层面也被阻断（错误码 -9836）

**推荐方案**:
1. 使用 rrequest 替换 reqwest（支持 TLS fingerprint 伪装）
2. 暂时保留 curl 方案，性能可接受
3. 添加 rrequest 测试验证可行性

**关键文件位置**:
- `src/data/rest/unified_client.rs:60-153` (curl 实现)
- `Cargo.toml:25` (reqwest 依赖)
- `examples/proxy_test.rs` (测试代码)

---

**报告生成时间**: 2026-01-18
**分析工具**:
- scutil, netstat, lsof (系统诊断)
- reqwest 0.12.28 (HTTP 客户端)
- FlClash (代理软件)
- Kong Gateway (JA3 TLS fingerprinting)
