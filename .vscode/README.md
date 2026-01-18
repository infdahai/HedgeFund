# VSCode Debug Configuration for opinion-hft

## Overview
This directory contains VSCode configuration files for debugging Rust code in the opinion-hft project.

## Files

### `launch.json` - Debug Configurations
Provides multiple debug scenarios:
- **Debug Main**: Debug the main program (`src/main.rs`)
- **Debug Unit Tests**: Debug library tests (`cargo test --lib`)
- **Debug All Tests**: Debug all tests including integration tests
- **Debug Integration Tests (live)**: Debug live integration tests (requires `.env`)
- **Debug Specific Test**: Prompt for test name to debug specific test
- **Debug Signer Benchmark**: Debug benchmark tests

### `tasks.json` - Build Tasks
Automated tasks for common operations:
- `cargo: build opinion-hft` - Build the main binary
- `cargo: check` - Run cargo check
- `cargo: clippy` - Run clippy linter
- `cargo: test (unit)` - Run unit tests only
- `cargo: test (all)` - Run all tests
- `cargo: test (integration)` - Run integration tests only
- `cargo: run` - Build and run the bot
- `cargo: bench` - Run benchmarks
- `cargo: clean` - Clean build artifacts
- `cargo: doc` - Generate and open documentation

### `settings.json` - Editor Settings
Rust-specific editor configurations:
- Auto-format on save
- Clippy as default checker
- Test thread configuration
- Terminal environment variables

### `extensions.json` - Recommended Extensions
Required VSCode extensions for Rust development:
- `rust-lang.rust-analyzer` - Rust language server
- `vadimcn.vscode-lldb` - Debugger (required for macOS/Linux)
- `serayuzgur.crates` - Cargo.toml dependency management
- `be5invis.toml` - TOML syntax highlighting
- `eamodio.gitlens` - Git integration
- `usernamehw.errorlens` - Inline error display

## Usage

### Installing Required Extensions

**macOS/Linux** (required for debugging):
```bash
code --install-extension rust-lang.rust-analyzer
code --install-extension vadimcn.vscode-lldb
```

**Windows** (use C++ debugger instead):
```bash
code --install-extension rust-lang.rust-analyzer
code --install-extension ms-vscode.cpptools
```

Install all recommended extensions:
```bash
code --install-extension $(cat extensions.json | jq -r '.recommendations[]')
```

### Debugging

#### 1. Debug Main Program
1. Open any Rust file in `src/`
2. Press `F5` or go to Run and Debug (Cmd+Shift+D)
3. Select "Debug Main"
4. Set breakpoints in your code
5. Start debugging

#### 2. Debug Unit Tests
1. Open a test file or source file
2. Set breakpoints in test code
3. Press `F5`
4. Select "Debug Unit Tests"

#### 3. Debug Integration Tests
1. Ensure `.env` file is configured with valid credentials
2. Open an integration test file (e.g., `tests/live_integration.rs`)
3. Set breakpoints
4. Press `F5`
5. Select "Debug Integration Tests (live)"

#### 4. Debug Specific Test
1. Press `F5`
2. Select "Debug Specific Test"
3. Enter the full test name when prompted (e.g., `tests::test_order_lifecycle`)
4. Breakpoints will only trigger in that specific test

### Building

Use the Command Palette (Cmd+Shift+P) and select "Tasks: Run Task":
- Choose `cargo: build opinion-hft` to build
- Choose `cargo: clippy` to lint
- Choose `cargo: test (unit)` to run unit tests

### Environment Variables

Debug configurations automatically set these environment variables:
- `RUST_LOG=debug` - Enable debug logging
- `RUST_BACKTRACE=1` - Enable backtraces on panic
- `OPINION_ENV=local` - Use local environment with proxy
- `OPINION_PROXY_URL=http://127.0.0.1:7890` - Proxy URL for TLS fingerprint bypass

**Proxy Configuration**:
All debug configurations are pre-configured to use a proxy (`http://127.0.0.1:7890` by default). This is required for local development to bypass Kong Gateway's TLS fingerprinting.

**Before debugging**:
1. Ensure your proxy software (e.g., FlClash, V2Ray) is running on port 7890
2. Verify proxy is accessible by checking `curl -x http://127.0.0.1:7890 https://proxy.opinion.trade:8443/openapi/health`

**To change proxy URL**:
- Modify `OPINION_PROXY_URL` in `.vscode/launch.json` (for debugger)
- Modify `OPINION_PROXY_URL` in `.vscode/settings.json` (for integrated terminal)

For live integration tests, ensure your `.env` file contains:
```env
OPINION_API_KEY=your_api_key
OPINION_REST_URL=https://proxy.opinion.trade:8443/openapi/
OPINION_WS_URL=wss://ws.opinion.trade
OPINION_ENV=local  # or aws
OPINION_PROXY_URL=http://127.0.0.1:7890  # only for local
BSC_RPC_URL=https://bsc-dataseed.binance.org
PRIVATE_KEY=0x...
OPINION_EOA_ADDRESS=0x...
```

## Troubleshooting

### Debugger not found on macOS
Install LLDB:
```bash
brew install lldb
```

### Debugger not found on Linux
Install LLDB:
```bash
sudo apt-get install lldb
```

### Breakpoints not hit
1. Ensure you're using `lldb` configuration (not `cppdbg`)
2. Check that the code is compiled in debug mode (`cargo build`, not `cargo build --release`)
3. Verify breakpoints are set in the right file

### Integration tests fail
1. Check that `.env` file exists in project root directory
2. Verify API credentials are valid
3. Ensure proxy is running if using `OPINION_ENV=local`

### Proxy connection errors
**Symptoms**: Connection timeout, DNS resolution failed, or 403 Forbidden errors

**Solutions**:
1. **Check proxy is running**:
   ```bash
   # Test proxy connectivity
   curl -x http://127.0.0.1:7890 https://proxy.opinion.trade:8443/openapi/health
   ```

2. **Verify proxy port**:
   - FlClash default: `http://127.0.0.1:7890` (HTTP) or `socks5://127.0.0.1:7890` (SOCKS5)
   - V2Ray default: `http://127.0.0.1:10809` (HTTP) or `socks5://127.0.0.1:10808` (SOCKS5)

3. **Update proxy URL in configuration**:
   - Edit `.vscode/launch.json` - update `OPINION_PROXY_URL` for debugger
   - Edit `.vscode/settings.json` - update `OPINION_PROXY_URL` for terminal

4. **Disable proxy for AWS environment**:
   - Set `OPINION_ENV=aws` to bypass proxy
   - AWS Tokyo region doesn't require proxy for Kong Gateway

5. **Check TLS fingerprinting**:
   - Kong Gateway uses JA3 TLS fingerprinting
   - Proxy is required on local machines to bypass this
   - Direct connections may be rejected with 403 Forbidden

## Keyboard Shortcuts

| Action | Shortcut |
|--------|----------|
| Start Debugging | `F5` |
| Continue | `F5` |
| Step Over | `F10` |
| Step Into | `F11` |
| Step Out | `Shift+F11` |
| Toggle Breakpoint | `F9` |
| Run Task | `Cmd+Shift+P` → "Tasks: Run Task" |
| Command Palette | `Cmd+Shift+P` |

## Additional Resources

- [VSCode Rust Debugging Guide](https://code.visualstudio.com/docs/rust/rust-debugging)
- [rust-analyzer Manual](https://rust-analyzer.github.io/manual.html)
- [LLDB Command Reference](https://lldb.llvm.org/use/map.html)
