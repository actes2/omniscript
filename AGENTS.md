# AGENTS.md — OmniScript (`omnic`) Engineering & Agent Operating Guide

> **Audience**: Any AI coding agent (Claude, Gemini, Codex, Cursor, Aider, etc.) or systems engineer working on `omnic`, extending its polyglot runtime/backends, retraining its GGUF synthesis model, or helping end-users compile OmniScript to desktop (`x86_64` / `aarch64`) and bare-metal Raspberry Pi Pico (`RP2040` / `RP2350`).

---

## 1. Core Philosophy & Non-Negotiable PR Policy

### What OmniScript (`omnic`) Is
**OmniScript is a universal polyglot compiler.** Its mission is to accept source written in **any programming language or mixed-language idiom** (Python, C, C#, Java, Rust, Go, JavaScript, Ruby, Kotlin, Swift, Arduino/C++, Lua, Zig, etc.), lower it to a unified internal SSA LLVM IR, and emit native machine code for desktop and bare-metal targets.

Ambiguities and foreign ecosystem calls that do not have a deterministic lowering are handled by an **in-process LLM mid-layer** (`models/omni-coder.gguf`, running via Candle on CPU/GPU), trained specifically on OmniScript's decorated `<omni_ir_task>` SSA contracts with Stage-1 masked SFT + Stage-2 ORPO contrastive preference optimization.

**The core thesis is dynamism, interoperability, and polymorphism — never pigeonholing the compiler into static, single-language or single-board silos.**

### Strict PR Rejection Rule (Read Before Writing Any Code)
**PRs that introduce unjustified static/hardcoded mechanisms into `omni_codegen` WILL BE REJECTED.**

Specifically:
- **FORBIDDEN**: Hardcoding board-specific, framework-specific, or foreign-language symbol names or string checks inside `crates/omni_codegen/src/lib.rs` (e.g., `if callee == "wifi_ip" { ... }`, hardcoding `gpio_put` / `ws_poll` in `NATIVE_C_LIB_FNS`, or hardcoding `Console.ReadLine` -> `input()` inside `omni_codegen`).
- **JUSTIFIED STATIC MECHANISMS (The Only Allowed Boundaries)**:
  1. **Target-independent universal runtime ABI symbols** (`@__omni_print_str`, `@__omni_print_int`, `@__omni_print_float`, `@__omni_input`, `@getchar`, `@putchar`, `@__omni_str_*`, `@__omni_slice_*`, `@__omni_map_*`, Perceus RC `@__omni_rc_*`).
  2. **First-class `extern fn` / `extern "C" { ... }` declarations** and **virtual target/HAL preludes** in `crates/omni_preamble/src/lib.rs` (`import "pico*"`, `--platform pico*`).
  3. **Standard ISO C libc FFI** (`NATIVE_C_LIB_FNS`).
- **REQUIRED PATTERN FOR EVERYTHING ELSE**:
  - Expose real target capabilities via `extern fn` preludes or universal runtime symbols.
  - Surface those symbols dynamically to the LLM via `<platform_abi>` and `__OMNI_AVAILABLE_SYM__`.
  - If the GGUF model fails to infer a new polyglot category, **expand `crates/omni_engine/src/dataset_generator.rs` and retrain `models/omni-coder.gguf`** rather than adding a static regex hack in `omni_codegen`.
  - Proof this works: after adding 4 generalized I/O & symbol-dispatch families and retraining, `omni-coder.gguf` lowered completely **unseen** foreign calls (`BufferedReader.readLine()`, `Serial.readString()`, `Console.ReadLine()`) zero-shot on the first pass (`0` retries, `temp = 0.00`) to `call i8* @__omni_input(i8* null)` on both `x86_64` Linux and bare-metal RP2350.

---

## 2. Workspace & Crate Map

| Crate | Path | Responsibility |
| :--- | :--- | :--- |
| `omni_cli` | `crates/omni_cli/src/main.rs` | `omnic` CLI (`build`, `run`, `repl`, `install`, `synth-dataset`), `--platform auto` inference from HAL imports, cache orchestration. |
| `omni_parser` | `crates/omni_parser/src/lib.rs` | Polyglot indentation/brace canonicalizer, recursive-descent + Pratt parser, `ExternFnDef` (`extern fn`, `extern "C" { ... }`, `declare fn`). |
| `omni_preamble` | `crates/omni_preamble/src/lib.rs` | Multi-ecosystem dependency driller (Python, Node, Rust, Go, C/C++, Java, C# `.NET BCL`, Lua, Zig), `oenv` virtual envs, `pioasm` assembler, virtual HAL preludes (`PICO_HAL_BASE_PRELUDE`, `PICO_HAL_FULL_PRELUDE`). |
| `omni_codegen` | `crates/omni_codegen/src/lib.rs` | Generic SSA IR generator, `ScopeTree`, Perceus RC, `emit_extern_fn_call`, `synthesize_unresolved_expr`, fail-closed `[OMNIC SEMANTIC ERROR]` checks. **Must remain 100% free of hardcoded hardware/board symbols.** |
| `omni_engine` | `crates/omni_engine/src/lib.rs`, `dataset_generator.rs` | In-process Candle GGUF inference engine, `<omni_ir_task>` + `<platform_abi>` prompt builder, IR self-repair loop, and Clang-verified 24-family synthetic dataset generator. |
| `omni_runtime` | `crates/omni_runtime/` | Desktop C runtime shim (`x86_64` / `aarch64` Linux, macOS, Windows) implementing `@__omni_print_*`, `@__omni_input`, strings, slices, maps, fibers, channels, Perceus RC. |
| `omni_mem` | `crates/omni_mem/` | Memory/lifetime & Perceus RC analysis helpers. |
| `omni_verify` | `crates/omni_verify/src/` | Desktop Clang IR verifier/linker (`lib.rs`) **AND** the 100% pure-Rust, zero-`clang` Cortex-M compiler backend (`cortex_m/`). |

### Key Cortex-M Backend Files (`crates/omni_verify/src/cortex_m/`)
- `chip.rs`: `RpChipProfile::rp2040()` (`thumbv6m-none-eabi`, `0xE48BFF56`, 256B CRC32 `boot2`) & `RpChipProfile::rp2350()` (`thumbv8m.main-none-eabihf`, `0xE48BFF59`, `PICOBIN IMAGE_DEF`).
- `ir_lower.rs`: Pure-Rust LLVM SSA IR -> ARMv6-M / ARMv8-M.main Thumb machine code lowerer.
- `thumb_enc.rs`: Pure-Rust Thumb / Thumb-2 assembler + relocation patcher.
- `elf_uf2.rs`: Standalone 32-bit ARM EABI5 `.elf` + 512B-block `.uf2` packager.
- `runtime_fw.rs`: `Reset_Handler` (VTOR, XOSC, PLL_SYS 125/150 MHz, PLL_USB 48 MHz, post-clock `USBCTRL` unreset, SRAM scratch zeroing, UART0, USB CDC init), heap allocator, math/float/string/slice/map/RC runtime, dual-console `putchar`/`puts`/`getchar`/`__omni_input`.
- `usb_cdc_fw.rs`: Freestanding USB 2.0 Full-Speed CDC-ACM (`/dev/ttyACM0`, `2e8a:000a`) ISR driver + hands-free 1200-baud BootROM `BOOTSEL` reboot.
- `cyw43_fw.rs`: Cycle-accurate 3-wire gSPI bit-bang primitives (`GP23=WL_ON`, `GP24=WL_DIO`, `GP25=WL_CS`, `GP29=WL_CLK`), CYW43439 A0 firmware/NVRAM/CLM blobs (`include_bytes!`), and `wifi_*`/`ws_*`/`tcp_*` entrypoints.
- `cyw43_ws_stack.omni` & `cyw43_ws_ir.ll`: **Self-hosted OmniScript** bare-metal OmniNet stack (SDPCM/BCDC, CLM load, WPA2-PSK join, Ethernet, ARP, IPv4, ICMP ping, DHCPv4, UDP `:1425` discovery beacon, TCP state machine, SHA-1, Base64, RFC 6455 WebSocket `:1424`, raw TCP socket server).
- `mod.rs`: Full pure-Rust ARMv6-M / ARMv8-M.main `ThumbCpuEmu` + wire-accurate `SimCyw43439` + USB DPRAM emulator test suite (16 tests).

---

## 3. Build, Test & Verification Commands

Always build and test in `--release` (the Cortex-M emulator tests execute tens of millions of simulated Thumb cycles and run in `< 0.5s` in release mode):

```bash
# Fast unit & integration verification across all core crates
cargo test --release -p omni_parser --lib
cargo test --release -p omni_preamble --lib
cargo test --release -p omni_codegen --lib
cargo test --release -p omni_engine --lib
cargo test --release -p omni_verify --lib

# Build release compiler binary
cargo build --release --bin omnic
```

### Compiling User Programs
```bash
# Desktop (auto-detects host x86_64/aarch64)
./target/release/omnic build --no-cache app.omni -o app

# Raspberry Pi Pico 2 W (RP2350 + CYW43439) -> emits BOTH .uf2 and .elf
./target/release/omnic build --platform pico2w --no-cache examples/pico2w_blink/blink_pico2w.omni -o examples/pico2w_blink/blink_pico2w.uf2

# Auto-platform inference (no --platform flag needed when importing a board HAL)
# `import "pico2w";` -> auto-selects thumbv8m.main-none-eabihf
# `import "pico";`   -> auto-selects thumbv6m-none-eabi
./target/release/omnic build --no-cache my_pico2w_app.omni -o my_pico2w_app.uf2
```

### Target / HAL Isolation Rules
| Target / Import | Exposed HAL Symbols | Wireless (`wifi_*`, `ws_*`, `tcp_*`, `cyw43_*`) |
| :--- | :--- | :--- |
| Desktop (`linux`/`macos`/`windows`), no `import "pico*"` | None (fails closed with `[OMNIC SEMANTIC ERROR]`) | Rejected |
| `--platform pico` or `--platform pico2` (non-wireless) | `PICO_HAL_BASE_PRELUDE` (GPIO, PIO, MMIO, `mem_read8`/`8`, timers, UART, `usb_cdc_getc`) | Rejected unless `import "pico/net"` or `import "pico"` |
| `--platform picow` or `--platform pico2w`, or `import "pico*"` | `PICO_HAL_FULL_PRELUDE` (base + `cyw43_*`, `wifi_*`, `ws_*`, `tcp_*`) | Enabled |

Both unqualified (`wifi_ip()`, `gpio_put(25, 1)`) and aliased (`import "pico2w" as hw; hw.wifi_ip()`) calls are supported identically.

---

## 4. Working with Raspberry Pi Pico Infrastructure (End-User & Agent Playbook)

### Zero-Dependency Bare-Metal Pipeline
Targeting `pico`, `picow`, `pico2`, or `pico2w` requires **zero external ARM toolchains** (`no arm-none-eabi-gcc`, `no clang`, `no pico-sdk`, `no lwIP`, `no cyw43_arch`). `omnic` lowers SSA IR directly to Thumb machine code and writes both `.uf2` (for `picotool` / BOOTSEL drag-and-drop) and `.elf` (for `rizin`, `objdump`, Ghidra, `openocd`).

### Flashing & Hands-Free 1200-Baud BOOTSEL
- **If the Pico is already running OmniScript firmware (`/dev/ttyACM0`, USB ID `2e8a:000a`)**:
  Trigger hands-free BOOTSEL without touching the physical button (a baud *transition* is required so Linux `cdc_acm` sends `SET_LINE_CODING(1200)`):
  ```bash
  stty -F /dev/ttyACM0 115200 && sleep 0.2 && stty -F /dev/ttyACM0 1200 && sleep 1.5
  picotool load -f -v -x path/to/firmware.uf2
  ```
- **If the board is unprogrammed or faulted**: Have the user hold **BOOTSEL** while plugging in USB (`2e8a:000f` RP2350 Boot), then run `picotool load -f -v -x path/to/firmware.uf2`.

### Serial Console (`/dev/ttyACM0`) & Host Echo Caveat
- All `print()`, `puts()`, `putchar()`, `uart_puts()`, `input()`, and `getchar()` calls automatically tee across **both** USB CDC-ACM (`/dev/ttyACM0`) and hardware `UART0` (`GP0`/`GP1`, `115200 8N1`).
- **CRITICAL LINUX CAVEAT**: Linux enables terminal `ECHO` on `/dev/ttyACM0` by default. If a script or user opens `/dev/ttyACM0` without disabling `ECHO`, the Linux kernel will echo the Pico's own boot banner straight back into the Pico's RX buffer (corrupting the first `input()` / `Console.ReadLine()` prompt!).
- Always instruct users to attach in raw, no-echo mode:
  ```bash
  stty -F /dev/ttyACM0 raw -echo 115200 && screen /dev/ttyACM0 115200
  ```
  (Or `picocom -b 115200 /dev/ttyACM0`.)

### Bare-Metal Networking (`examples/pico2w_tcp_server/tcp_pico2w.omni` & `examples/pico2w_websocket/ws_pico2w.omni`)
- **ICMP Ping** works automatically whenever `tcp_poll()` or `ws_poll()` is being pumped.
- **UDP Discovery Beacon**: Broadcasts `"OMNI_PICO2W ip=<ip> ws_port=<port>"` on UDP port `1425` every 2 seconds so LAN clients can discover the DHCP IPv4 address.
- **Raw TCP Socket Server**: `tcp_listen(port)`, `tcp_poll() -> str`, `tcp_send(msg)`, `tcp_is_open()`, `tcp_close()`.
- **RFC 6455 WebSocket Server**: `ws_listen(port)`, `ws_poll() -> str`, `ws_send(msg)`, `ws_is_open()`.

---

## 5. Critical Compiler & Bare-Metal Hardware Caveats

Every agent touching `omni_codegen` or `crates/omni_verify/src/cortex_m/` MUST observe these invariants:

1. **Cortex-M 64-Bit Register-Pair `extern fn` ABI**:
   - Compiled OmniScript functions (`@__omni_fn_*`) pass every parameter as a 64-bit `i64` register pair (`r0:r1` for arg 0, `r2:r3` for arg 1, stack for args 2+).
   - Therefore `emit_extern_fn_call` coerces all `extern fn` arguments via `coerce_to_raw_i64_bits` (`ptrtoint i8* ... to i64` for strings) while preserving exact LLVM return types (`i8*` for `str`, `double` for `float`, `void` for `None`, `i32`/`i64` for ints). Passing `i8*` directly as 32-bit `r0`/`r1` breaks 2-arg calls like `wifi_connect(ssid, pass)` on ARMv8-M.
2. **RP2350 Cold Power-On Reset (POR) vs. Warm Reboot**:
   - **Post-clock `USBCTRL` unreset**: On cold POR, `CLK_USB` is disabled (0 MHz). `Reset_Handler` MUST hold `USBCTRL` in reset until `PLL_USB` locks and 48.000 MHz `CLK_USB` is enabled (Step D3 -> Step D4), then unreset `USBCTRL` and poll `RESET_DONE`. Unresetting `USBCTRL` before `CLK_USB` is active causes the D+ pull-up to trigger `new full-speed USB device` while every `SET_ADDRESS`/`GET_DESCRIPTOR` times out (`error -62` / `-110`).
   - **Cold `XOSC` startup**: Use `STARTUP = 188` (~4 ms) and poll `CLK_REF_SELECTED` (`+0x38`) after switching `CLK_REF` to `XOSC`.
   - **Cold SRAM zeroing**: Physical SRAM at POR contains random garbage (unlike `ThumbCpuEmu` which starts zeroed). `Reset_Handler` MUST zero `0x2000_000C..0x2000_001F` (especially `net_ctx_ptr` at `0x2000_001C`); otherwise `cyw43_gpio_put` sees non-zero `net_ctx_ptr` on cold boot and faults, leaving the onboard LED dark.
3. **Editing `cyw43_ws_stack.omni` Requires Regenerating `cyw43_ws_ir.ll`**:
   - `cyw43_fw.rs` embeds `include_str!("cyw43_ws_ir.ll")`. Whenever you edit `crates/omni_verify/src/cortex_m/cyw43_ws_stack.omni`, recompile it with `omnic build --platform pico2w --cache-dir <tmp>` and transform the emitted `.ll` into `cyw43_ws_ir.ll`:
     - Strip `; ModuleID`, `target triple`, `target datalayout`, `source_filename`.
     - Strip `define i64 @__omni_fn_main` and `define i32 @main`.
     - Namespace `@.str.omni_` / `@.str.` -> `@.str.cyw_`.
     - Namespace internal `@__omni_fn_*` -> `@__omni_cyw_priv_*` **except** exported `@__omni_fn_cyw_*`.
4. **Perceus RC & LLM String Promotion in `omni_codegen`**:
   - Always slice `deferred_frees` defensively (`deferred_frees.get(snapshot..)`) on branch/loop exits because an inner `return` may consume an outer owned binding.
   - When LLM synthesis produces a pointer/string (`call i8* @__omni_input`, `call i8* @__omni_str_*`, or `ptrtoint i8*`), promote the `ScopeTree` binding to `i8* (string)` so `print(x)`, `puts(x)`, `strlen(x)`, and `"hi " + x` work identically on both desktop and Cortex-M.
5. **Zero-Secrets Hygiene (NEVER Commit Wi-Fi Credentials)**:
   - Never hardcode real Wi-Fi SSIDs/passphrases in `examples/`, unit tests, or `.omnicache`. Use interactive `Console.ReadLine()` / `input()` (volatile SRAM only) or `"YOUR_WIFI_SSID"` / `"YOUR_WIFI_PASSWORD"` placeholders.
   - Keep `*.uf2`, `*.elf`, `models/*.gguf`, and `tests/outputs/*` gitignored.

---

## 6. LLM Mid-Layer & Retraining Workflow

When adding a new class of polyglot idioms that the LLM mid-layer should synthesize dynamically:

1. **Expose the runtime / HAL symbols** in `<platform_abi>` (`crates/omni_engine/src/lib.rs`) or via `__OMNI_AVAILABLE_SYM__:` (`crates/omni_codegen/src/lib.rs`) so inference-time prompts show the exact available symbols.
2. **Add a Clang-verified parametric family** to `crates/omni_engine/src/dataset_generator.rs` (including an ORPO `rejected_completion` targeting the specific failure mode, such as comment-loop collapse or hallucinated undefined symbols) and verify with `cargo test --release -p omni_engine --lib`.
3. **Regenerate the dataset & `imatrix` calibration**:
   ```bash
   ./target/release/omnic synth-dataset --count 6000 --output tests/outputs/synthetic_training_dataset.jsonl
   ```
4. **Distribute to GPU workstation & retrain** (Stage-1 completion-masked SFT + Stage-2 ORPO + `llama-imatrix` + `Q4_K_M`, see `docs/GPU_TRAINING_WORKFLOW.md`):
   - Host locally: `python3 scripts/serve_training_assets.py` (`GET /bundle.tar.gz`, `PUT /upload`).
   - Train on GPU: `python3 train_omni_model.py --base_model Qwen/Qwen2.5-Coder-1.5B --dataset synthetic_training_dataset.jsonl --output_dir ./omni_qwen_1.5b --epochs 3 --orpo_epochs 1`.
   - Convert & quantize with `llama.cpp` (`convert_hf_to_gguf.py` -> `llama-imatrix` -> `llama-quantize Q4_K_M`), then push back via `curl -T omni-coder.gguf http://<host>:8080/upload`.
5. **Verify zero-shot generalization** using **unseen** foreign calls not present in `dataset_generator.rs` (e.g., `BufferedReader.readLine()`, `Serial.readString()`) on both `--platform pico2w` and native desktop `x86_64`.

---

## 7. Pre-Commit / PR Checklist
- [ ] **No hardcoded hardware/board/framework names in `crates/omni_codegen/src/lib.rs`** (outside `#[cfg(test)]`).
- [ ] **Follows existing standardized format** we work in giant files for a reason, there's little to no-abstraction; this is intentional for various reasons
- [ ] `cargo test --release -p omni_parser --lib && cargo test --release -p omni_preamble --lib && cargo test --release -p omni_codegen --lib && cargo test --release -p omni_engine --lib && cargo test --release -p omni_verify --lib` all pass.
- [ ] Zero real credentials/secrets in tracked files or examples; no `.uf2`/`.elf`/`.gguf` binaries staged.
- [ ] If `cyw43_ws_stack.omni` changed, `cyw43_ws_ir.ll` was regenerated and verified via `test_cyw43439_*`.
- [ ] If `runtime_fw.rs` changed, cold-boot ordering and SRAM scratch invariants pass `test_pll_sys_clock_init_*` and `test_usb_cdc_acm_*`.
