# Edge Runtime 実装レポート

## 概要

Rust + Wasmtime を使用した高密度・低レイテンシなサーバーレス・エッジランタイムの実装。
WebAssembly Component Model に準拠し、安全で高速なサーバーレス実行環境を提供する。

| 項目 | 値 |
|------|-----|
| Rust Edition | 2024 |
| MSRV | 1.85.0 |
| Wasmtime Version | 28 |
| Interface | WIT + Component Model |

---

## プロジェクト構造

```
edge-runtime/
├── Cargo.toml                    # Workspace root
├── rust-toolchain.toml           # Rust 1.85.0 stable
├── rustfmt.toml                  # フォーマット設定
├── clippy.toml                   # Lint設定
├── deny.toml                     # 依存関係チェック
│
├── crates/
│   ├── edge-runtime-common/      # 共通型・エラー・設定
│   ├── edge-runtime-core/        # Wasmtime Engine/Store/Instance
│   └── edge-runtime-host/        # Host Functions (WIT-based)
│
├── wit/                          # WIT interface definitions
│   ├── world.wit
│   └── interfaces/
│       ├── logging.wit
│       └── http-outbound.wit
│
└── .github/workflows/ci.yml      # CI/CD
```

---

## Phase 1: Foundation Setup

### 目標
Workspace構造、共通型、CI/CD の基盤構築

### 実装内容

#### 1.1 Workspace設定 (`Cargo.toml`)

```toml
[workspace]
resolver = "2"
members = [
    "crates/edge-runtime-common",
    "crates/edge-runtime-core",
    "crates/edge-runtime-host",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
license = "MIT OR Apache-2.0"
```

**主要な依存関係:**
| パッケージ | バージョン | 用途 |
|-----------|-----------|------|
| wasmtime | 28 | Wasm実行エンジン |
| tokio | 1.43 | 非同期ランタイム |
| thiserror | 2.0 | エラー型定義 |
| serde | 1.0 | シリアライゼーション |
| tracing | 0.1 | 構造化ログ |

#### 1.2 edge-runtime-common クレート

**error.rs - エラー型定義**

```rust
#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Module not found: {module_id}")]
    ModuleNotFound { module_id: String },

    #[error("Compilation failed: {reason}")]
    CompilationFailed { reason: String },

    #[error("Execution timeout after {duration_ms}ms")]
    ExecutionTimeout { duration_ms: u64 },

    #[error("Fuel exhausted")]
    FuelExhausted,

    #[error("Memory limit exceeded: {limit_mb}MB")]
    MemoryLimitExceeded { limit_mb: u32 },

    #[error("Host function error: {0}")]
    HostFunction(#[from] HostFunctionError),

    #[error("WASI error: {0}")]
    Wasi(#[from] WasiError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Trap: {message}")]
    Trap { message: String },

    #[error("Invalid configuration: {reason}")]
    InvalidConfig { reason: String },
}
```

**HostFunctionError:**
- `HttpRequestFailed { url, status }` - HTTP失敗
- `PermissionDenied { resource }` - 権限不足
- `KvStore(String)` - KVストアエラー
- `RateLimitExceeded { operation }` - レート制限
- `InvalidArgument { reason }` - 不正引数

**config.rs - 設定構造体**

```rust
/// トップレベル設定
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
}

/// エンジン設定
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EngineConfig {
    pub pooling_allocator: bool,      // デフォルト: true
    pub max_instances: u32,            // デフォルト: 1000
    pub instance_memory_mb: u32,       // デフォルト: 64
    pub cache_compiled_modules: bool,  // デフォルト: true
    pub cache_dir: Option<String>,
    pub epoch_interruption: bool,      // デフォルト: true
}

/// 実行設定
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionConfig {
    pub max_fuel: u64,         // デフォルト: 10,000,000
    pub timeout_ms: u64,       // デフォルト: 100
    pub max_memory_mb: u32,    // デフォルト: 128
    pub fuel_metering: bool,   // デフォルト: true
}
```

#### 1.3 CI/CD パイプライン

```yaml
# .github/workflows/ci.yml
jobs:
  check:     # cargo fmt + clippy
  test:      # cargo test (Ubuntu/macOS)
  security:  # cargo-audit
  deny:      # cargo-deny (ライセンス/脆弱性)
  docs:      # cargo doc
```

**Clippy設定 (clippy.toml):**
- `cognitive-complexity-threshold = 25`
- `too-many-lines-threshold = 150`
- `too-many-arguments-threshold = 8`

**cargo-deny設定 (deny.toml):**
- 許可ライセンス: MIT, Apache-2.0, BSD, ISC, Zlib
- 対象プラットフォーム: x86_64-linux, x86_64-darwin, aarch64-darwin

---

## Phase 2: Runtime Core (Wasmtime Integration)

### 目標
Wasmtime Engine/Store/Instance管理、Pooling Allocator、Fuel Metering

### アーキテクチャ

```
WasmEngine (共有、スレッドセーフ)
    ↓
CompiledModule (キャッシュされた機械語)
    ↓
Store<WorkerContext> + Instance (リクエスト毎、隔離)
    ↓
ExecutionResult (メトリクスを含む)
```

### 実装内容

#### 2.1 engine.rs - WasmEngine

```rust
pub struct WasmEngine {
    engine: Arc<Engine>,
    config: EngineConfig,
}
```

**主要メソッド:**
| メソッド | 説明 |
|---------|------|
| `new(config)` | エンジン初期化（Pooling Allocator設定含む） |
| `inner()` | 内部Engineへのアクセス |
| `increment_epoch()` | エポックカウンタ増分 |
| `is_pooling_enabled()` | プーリング有効確認 |

**設定内容:**
```rust
wasmtime_config.async_support(true);      // 非同期対応
wasmtime_config.consume_fuel(true);       // Fuel計測
wasmtime_config.epoch_interruption(true); // エポック割り込み
wasmtime_config.cranelift_opt_level(OptLevel::Speed);

// Pooling Allocator
let pooling = PoolingAllocationConfig::default()
    .total_component_instances(max_instances)
    .total_core_instances(max_instances)
    .total_memories(max_instances)
    .total_tables(max_instances)
    .max_memory_size(max_memory_bytes);
```

**パフォーマンス改善:**
| 構成 | インスタンス作成時間 |
|-----|---------------------|
| Pooling allocator無し | ~1ms |
| Pooling allocator有り | ~10µs |
| **改善率** | **100倍** |

#### 2.2 store.rs - WorkerContext

```rust
pub struct WorkerContext {
    wasi: WasiCtx,
    table: ResourceTable,
    pub request_id: String,
    pub logs: Vec<LogEntry>,
    pub metrics: ExecutionMetrics,
    start_time: Instant,
}
```

**ログエントリ:**
```rust
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: Instant,
}

pub enum LogLevel { Debug, Info, Warn, Error }
```

**メトリクス:**
```rust
pub struct ExecutionMetrics {
    pub fuel_consumed: u64,
    pub memory_used_bytes: usize,
    pub duration: Option<Duration>,
}
```

**ヘルパー関数:**
| 関数 | 説明 |
|------|------|
| `create_store(engine, config, request_id)` | Fuel/エポック制限付きStore作成 |
| `get_remaining_fuel(store)` | 残りFuel取得 |
| `calculate_fuel_consumed(initial, store)` | 消費Fuel計算 |

#### 2.3 module.rs - CompiledModule

```rust
pub struct CompiledModule {
    inner: ModuleKind,
    content_hash: String,
    compiled_at: Instant,
}

enum ModuleKind {
    Core(Module),
    Component(Component),
}
```

**コンパイル方法:**
| メソッド | 用途 |
|---------|------|
| `from_bytes(engine, bytes)` | Wasmバイナリから |
| `from_component_bytes(engine, bytes)` | Componentから |
| `from_wat(engine, wat)` | WATソースから |
| `from_precompiled(engine, path)` | AOTコンパイル済みから |

**バリデーション:**
```rust
fn validate_wasm_header(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..4] == [0x00, 0x61, 0x73, 0x6D]  // \0asm
}
```

#### 2.4 instance.rs - InstanceRunner

```rust
pub struct InstanceRunner {
    engine: Arc<Engine>,
    linker: Linker<WorkerContext>,
    component_linker: ComponentLinker<WorkerContext>,
}

pub enum ExecutionResult {
    Success,
    Trap { message: String, code: Option<String> },
}
```

**実行フロー:**
1. `linker.instantiate_async()` でインスタンス化
2. `get_typed_func::<(), ()>(entry_point)` でエントリポイント取得
3. `call_async()` で非同期実行
4. Fuel消費量を記録
5. トラップ検出と処理

---

## Phase 3: Host Functions (WIT-based)

### 目標
WIT定義によるHost Functions実装（Logging, HTTP Outbound）

### WIT定義

#### world.wit
```wit
package edge:runtime@0.1.0;

world edge-worker {
    import logging;
    import http-outbound;
    export run: func() -> result<_, string>;
}

world http-handler {
    import logging;
    import http-outbound;
    export handle: func(request: http-request) -> result<http-response, string>;
}
```

#### interfaces/logging.wit
```wit
interface logging {
    enum log-level { debug, info, warn, error }

    log: func(level: log-level, message: string);
    debug: func(message: string);
    info: func(message: string);
    warn: func(message: string);
    error: func(message: string);
}
```

#### interfaces/http-outbound.wit
```wit
interface http-outbound {
    enum method { get, head, post, put, delete, patch, options }

    record request {
        method: method,
        uri: string,
        headers: list<tuple<string, string>>,
        body: option<list<u8>>,
        timeout-ms: option<u32>,
    }

    record response {
        status: u16,
        headers: list<tuple<string, string>>,
        body: list<u8>,
    }

    enum http-error {
        permission-denied, timeout, dns-error,
        connection-failed, tls-error, body-too-large,
        rate-limited, other,
    }

    fetch: func(req: request) -> result<response, http-error>;
    get: func(uri: string) -> result<list<u8>, http-error>;
}
```

### 実装内容

#### 3.1 permissions.rs - 権限管理

```rust
pub struct Permissions {
    pub allowed_http_hosts: HashSet<String>,
    pub http_enabled: bool,
    pub max_http_requests: u32,
    pub logging_enabled: bool,
}
```

**パターンマッチング:**
| パターン | 例 | マッチ対象 |
|---------|-----|----------|
| 完全一致 | `api.example.com` | `api.example.com` のみ |
| ワイルドカード | `*.example.com` | `api.example.com`, `www.example.com` |
| すべて許可 | `*` | すべてのホスト（開発用） |

**SSRF保護:**
```rust
pub fn is_private_address(url: &str) -> bool {
    // ブロック対象:
    // - localhost, 127.0.0.1, [::1]
    // - Private ranges: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
    // - Link-local: 169.254.0.0/16
    // - Cloud metadata: 169.254.169.254, metadata.google.internal
}
```

**ビルダーパターン:**
```rust
let perms = Permissions::builder()
    .allow_http_hosts(["api.example.com", "*.internal.example.com"])
    .max_http_requests(50)
    .enable_logging()
    .build();
```

#### 3.2 logging.rs - ロギング実装

```rust
pub struct LoggingHost;

impl LoggingHost {
    pub fn log(ctx: &mut WorkerContext, level: LogLevel, message: &str) {
        ctx.logs.push(LogEntry { level, message, timestamp });

        match level {
            LogLevel::Debug => debug!("{}", message),
            LogLevel::Info => info!("{}", message),
            LogLevel::Warn => warn!("{}", message),
            LogLevel::Error => error!("{}", message),
        }
    }
}
```

**便利関数:** `log_debug`, `log_info`, `log_warn`, `log_error`

#### 3.3 http_outbound.rs - HTTP実装

```rust
pub struct HttpOutboundHost {
    client: Client,
    permissions: Permissions,
    request_count: AtomicU32,
}
```

**セキュリティチェック（実行順）:**
1. レート制限チェック
2. ホスト許可チェック
3. SSRF保護（プライベートアドレス検出）
4. リクエスト実行

**クライアント設定:**
```rust
let client = Client::builder()
    .timeout(Duration::from_secs(30))
    .connect_timeout(Duration::from_secs(10))
    .pool_max_idle_per_host(10)
    .user_agent("edge-runtime/0.1.0")
    .build()?;
```

---

## テスト戦略

### ユニットテスト

| クレート | テスト数 | 内容 |
|---------|---------|------|
| edge-runtime-common | 8 | エラー変換、設定シリアライズ |
| edge-runtime-core | 12 | エンジン作成、Fuel計測、モジュール検証 |
| edge-runtime-host | 16 | 権限、ロギング、HTTP、SSRF |

### 統合テスト

`crates/edge-runtime-core/tests/integration.rs`:

| テスト | 検証内容 |
|-------|---------|
| `test_basic_execution` | 基本的なWasm実行 |
| `test_fuel_consumption` | Fuel消費追跡 |
| `test_fuel_exhaustion` | 無限ループでのFuel枯渇 |
| `test_host_function_logging` | Host Function呼び出し |
| `test_trap_unreachable` | Trap検出 |
| `test_multiple_logs` | 複数レベルログ |

**テスト用WAT例:**
```wat
(module
    (import "env" "log" (func $log (param i32 i32 i32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "Hello from Wasm")

    (func (export "_start")
        (call $log (i32.const 1) (i32.const 0) (i32.const 15))
    )
)
```

---

## セキュリティモデル

### 権限管理（Capability-Based）

- **最小権限の原則**: デフォルトで全て拒否
- **明示的有効化**: ビルダーパターンで段階的に設定
- **隔離**: リクエスト毎の独立したコンテキスト

### リソース制限

| リソース | デフォルト値 | 設定項目 |
|---------|-------------|---------|
| CPU (Fuel) | 10,000,000 | `max_fuel` |
| メモリ | 128 MB | `max_memory_mb` |
| タイムアウト | 100 ms | `timeout_ms` |
| HTTPリクエスト | 100/実行 | `max_http_requests` |

### SSRF保護

```
ブロック対象:
├── Localhost: 127.0.0.0/8, ::1
├── Private: 10.x.x.x, 172.16-31.x.x, 192.168.x.x
├── Link-local: 169.254.0.0/16
└── Cloud metadata: 169.254.169.254
```

---

## 設計パターン

### 1. エラーハンドリング
thiserror + anyhow パターン。ライブラリ層で型付きエラー、アプリ層で柔軟なハンドリング。

### 2. 設定管理
Serde対応でTOML/JSONから直接ロード。デフォルト値は関数で定義。

### 3. ビルダーパターン
権限設定で採用。`#[must_use]` で未使用警告。

### 4. コンテキスト管理
リクエスト毎の `WorkerContext` で完全隔離。

### 5. 非同期実行
Tokioベースの非同期ホスト関数呼び出し。

---

## 実行方法

```bash
# 全テスト実行
cargo test --workspace

# 統合テスト実行
cargo test -p edge-runtime-core --test integration

# Clippy
cargo clippy --workspace -- -D warnings

# フォーマット
cargo fmt --check
```

---

## Next Steps (Post-MVP)

| Phase | 内容 |
|-------|------|
| Phase 4 | HTTP Server (Axum) + Request Handling |
| Phase 5 | Control Plane (Module Cache, AOT Compilation) |
| Phase 6 | Observability (Prometheus Metrics, OpenTelemetry) |
