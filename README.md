# AetherScript 🌌

A zero-dependency, stack-based **Virtual Machine** and **Bytecode Compiler** engineered completely from scratch in **Rust**.

Developed as an academic supercurricular project to explore programming language theory (PLT), low-level virtual hardware simulation, and context-free grammar constraints. AetherScript skips surface-level tree-walking interpretation by flattening high-level syntactic trees into a linear stream of compact bytecode primitives.

## 🏛️ Advanced Architecture Pipeline

```text
  📥 Raw Input String (.ae)
            │
            ▼
┌─────────────────────────┐
│ 1. Hand-Written Lexer   │ ──> Tokenizes character streams with multi-char lookahead
└─────────────────────────┘
            │
            ▼
┌─────────────────────────┐
│ 2. Recursive Parser     │ ──> Builds AST enforcing math operator precedence
└─────────────────────────┘
            │
            ▼
┌─────────────────────────┐
│ 3. Bytecode Compiler    │ ──> Flattens AST into linear arrays of custom OpCodes
└─────────────────────────┘
            │
            ▼
┌─────────────────────────┐
│ 4. Stack-Based VM Core  │ ──> Executes bytes via virtual Instruction Pointer (ip)
└─────────────────────────┘
            │
            ▼
  🖥️ Native Execution Output (Stdout)
```