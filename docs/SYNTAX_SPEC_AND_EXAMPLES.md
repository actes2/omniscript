# OmniScript: Syntax Specification & Polyglot Examples

## 1. Syntax Philosophy: "Pythonic Core, Omnivorous Reach"

OmniScript uses a **Pythonic baseline** as its foundation:
- Clean indentation or optional braces for block scoping.
- Classes modeled after Python's multi-faceted class system (`class`, `def`, `self`, `@decorators`, dunders).
- Dynamic, duck-typed, or strictly typed variables at the engineer's discretion.
- **Omnivorous ingestion**: Write C pointer logic, Python comprehensions, JavaScript arrow functions, or raw intent pseudo-code in the same function—as long as the operational logic is consistent, the compiler lowers it into typed, native LLVM IR.

---

## 2. Real-World Code Examples

### Example 1: Systems Data Pipeline (Pythonic Class with C Pointer Arithmetic)
Demonstrating how a Python class structure manages low-level memory and SIMD-speed execution without interpreter overhead.

```python
# omni: telemetry_pipeline.omni

@dataclass
class TelemetryFilter:
    min_temp: f32 = 20.0
    max_temp: f32 = 85.0
    drop_count: u64 = 0

    def process_buffer(self, raw_buffer: *const u8, count: usize) -> list[u32]:
        """
        Parses raw binary packets, filters by temperature range,
        and returns list of anomalous device IDs.
        """
        anomalies = []
        
        # Low-level C pointer traversal inside pythonic loop
        for i in range(count):
            # Casting raw byte offset directly to packet layout
            pkt = cast(*const PacketHeader, raw_buffer + (i * sizeof(PacketHeader)))
            
            # Polyglot logic check:
            if pkt.temp < self.min_temp or pkt.temp > self.max_temp:
                self.drop_count += 1
                anomalies.append(pkt.device_id)
                
        return anomalies
```

#### What OmniScript Lowers This To (LLVM IR):
```llvm
; Monomorphized struct representation
%struct.TelemetryFilter = type { float, float, i64 }
%struct.PacketHeader = type { i32, float, i16 }
%struct.ListU32 = type { i32*, i64, i64 }

define void @TelemetryFilter_process_buffer(%struct.TelemetryFilter* %self, i8* %raw_buffer, i64 %count, %struct.ListU32* %result) {
entry:
  ; LLVM lowers range() and pointer arithmetic into vectorized SIMD loops
  ; Field accesses are direct offset loads (%self + 0, %self + 4, etc.)
  ; Zero dynamic dictionary lookups.
  ret void
}
```

---

### Example 2: Mixing JavaScript/Functional Lambdas & Pythonic Dicts

```python
class RequestRouter:
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.routes = {}

    def register(self, path: str, handler):
        self.routes[path] = handler

    def dispatch(self, request):
        # JS arrow-style pipeline mixed with pythonic dict lookup
        tokens = request.headers.get("Authorization", "")
            .split(" ")
            .filter(t => t.length > 0)
        
        if not tokens or tokens[0] != "Bearer":
            return {"status": 401, "body": "Unauthorized"}
            
        handler = self.routes.get(request.path)
        if handler:
            return handler(request)
        return {"status": 404, "body": "Not Found"}
```

---

### Example 3: Context Managers (Python Dunders for RAII Memory Management)

```python
class DMAChannel:
    def __init__(self, channel_id: u8, buffer_size: usize = 4096):
        self.channel_id = channel_id
        self.buffer_size = buffer_size
        self.dma_handle = null

    def __enter__(self):
        # Acquired on enter (maps to native hardware lock/alloc)
        self.dma_handle = hardware_dma_acquire(self.channel_id, self.buffer_size)
        return self

    def __exit__(self, exc_type, exc_val):
        # Guaranteed cleanup at scope exit (compiled to LLVM landingpad / cleanup)
        if self.dma_handle:
            hardware_dma_release(self.dma_handle)

    def transmit(self, src_data: *const u8, len: usize):
        # Pure intent expression:
        intent: trigger non-blocking DMA burst from src_data to self.dma_handle, return bytes written
```

---

### Example 4: Intent & Pseudo-Logic Blocks
When the engineer doesn't care about the implementation details of a standard algorithm:

```python
class MetricAggregator:
    def summarize(self, metrics: list[MetricEvent]) -> ClusterReport:
        intent {
            group metrics by region and tier;
            compute p50, p95, and p99 latencies for each group;
            flag any group where error_rate > 0.01;
            return populated ClusterReport struct;
        }
```

---

## 3. Grammar Specification (EBNF Draft)

The grammar builds upon Python's indentation-based lexical structure (with optional brace-delimiters permitted):

```ebnf
Module            ::= ( ClassDef | FunctionDef | GlobalStmt )* ;

ClassDef          ::= Decorator* "class" Identifier ("(" BaseList? ")")? ":" Suite ;
FunctionDef       ::= Decorator* "def" Identifier "(" ParamList? ")" ("->" Type)? ":" Suite ;

ParamList         ::= Param ("," Param)* ;
Param             ::= ("*" | "**")? Identifier (":" Type)? ("=" Expr)? ;

Suite             ::= SimpleStmt | NEWLINE INDENT Statement+ DEDENT | "{" Statement* "}" ;

Statement         ::= AssignStmt | IfStmt | ForStmt | WhileStmt | ReturnStmt | ExprStmt | IntentStmt ;

AssignStmt        ::= Target (":" Type)? "=" Expr ;
IntentStmt        ::= ("intent" ":" Suite) | ("intent" "{" IntentBody "}") ;

Expr              ::= LogicalOrExpr | LambdaExpr | ListComp | ArrowFunc ;
ArrowFunc         ::= ("(" ParamList? ")" | Identifier) "=>" (Expr | "{" Statement* "}") ;
ListComp          ::= "[" Expr "for" Identifier "in" Expr ("if" Expr)? "]" ;

Type              ::= PrimitiveType | PointerType | GenericType | Identifier ;
PrimitiveType     ::= "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" | "bool" | "usize" ;
PointerType       ::= ("*const" | "*mut") Type ;
GenericType       ::= Identifier "[" (Type ("," Type)*)? "]" ;
```
