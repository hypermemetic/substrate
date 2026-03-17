# TDD Node v2 Implementation Plan

This document replaces `tdd-node.tickets.md`. It incorporates two design
improvements over v1:

1. **`BehavioralSpec` and `ExecutionContext` are separate structs.** The spec
   agent outputs abstract behavioral invariants only — no file paths, no shell
   commands. A dedicated project analysis agent derives `ExecutionContext`
   independently during `ContractValidating`. `TddContractArtifact` is their
   composition and is the shared source of truth for all downstream agents.

2. **`ContractValidating` phase is implemented.** Directly mirrors
   `DispatchTdd.tla` line 37. A semantic spec review agent checks the
   `BehavioralSpec` for internal consistency before branching. If inconsistent,
   `spec_cycle` is incremented and the spec agent reruns. This phase runs in
   parallel with `dispatch_execution_context`.

The TDD node is an `OrchaNodeKind::Tdd` variant in the orcha activation,
dispatched by `dispatch_node` in `graph_runner.rs`. It uses existing primitives:
`dispatch_task`, `dispatch_validate`, `dispatch_review`, and `Pm` storage.
The parent lattice graph sees a single opaque node.

TLA+ phase mapping:
```
ContractPhase      → TDD-3: dispatch_contract → BehavioralSpec
ContractValidating → TDD-4: dispatch_spec_review + dispatch_execution_context
Branching          → TDD-5: dispatch_tdd_branches (parallel impl + test)
Validating         → TDD-5: dispatch_validate
Repairing          → TDD-6: dispatch_tdd_repair + repair loop
EscalatingToHuman  → TDD-6: dispatch_review (loopback gate)
```

---

# TDD-1: Types — BehavioralSpec, ExecutionContext, SpecReview, TddContractArtifact, OrchaNodeKind::Tdd [agent]

Add all new types to `src/activations/orcha/types.rs`, update `OrchaNodeKind`,
`OrchaNodeSpec`, and add `add_tdd` builder to `OrchaGraph`.

## Changes to src/activations/orcha/types.rs

### BehavioralSpec

Abstract, analyzable. Output of the spec agent. No file paths, no shell commands.
Maps directly to DispatchTdd.tla `contract = "present"`.

```rust
/// Abstract behavioral spec — the shared source of truth for impl and test agents.
/// Deliberately excludes all implementation details (no file paths, no commands).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BehavioralSpec {
    /// What must be true before calling the function/module.
    pub preconditions: Vec<String>,
    /// What must be true after calling the function/module.
    pub postconditions: Vec<String>,
    /// Invariants that hold across all inputs — suitable for property-based tests.
    pub properties: Vec<String>,
    /// Concrete {input, expected} pairs for unit tests.
    pub examples: Vec<serde_json::Value>,
    /// Boundary and edge cases that must be explicitly handled.
    pub edge_cases: Vec<String>,
    /// Behaviors explicitly excluded from this spec (prevents scope creep).
    pub out_of_scope: Vec<String>,
}
```

### ExecutionContext

Mechanical, project-specific. Derived by a project analysis agent during
`ContractValidating`, independent of the spec agent.

```rust
/// Project-specific execution context — derived by the project analysis agent.
/// Tells impl and test agents where to write code and how to validate it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecutionContext {
    /// Source files the impl agent should modify.
    pub impl_targets: Vec<String>,
    /// Path where the test agent must write its test file.
    pub test_path: String,
    /// Shell command that validates the implementation against the tests (exit 0 = pass).
    pub validate_command: String,
    /// Test framework in use, e.g. "cargo test", "pytest", "jest".
    /// Drives property-based test import style (proptest / hypothesis / fast-check).
    pub test_framework: String,
}
```

### SpecReview

Output of the semantic spec review agent. Interface is designed to accommodate
a future TLC-based replacement without changing `dispatch_tdd`'s control flow.

```rust
/// Result of the ContractValidating phase.
/// Consistent = true → proceed to Branching.
/// Consistent = false → increment spec_cycle, return to ContractPhase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecReview {
    /// Whether the spec is internally consistent and ready to branch.
    pub consistent: bool,
    /// Issues found (empty if consistent).
    pub issues: Vec<String>,
    /// Non-blocking suggestions (may be empty).
    pub suggestions: Vec<String>,
    /// Review method: "semantic_review" | "tlc" (for observability).
    pub method: String,
}
```

### TddContractArtifact

Composition of `BehavioralSpec` + `ExecutionContext`. Produced at the end of
`ContractValidating`. This is what impl and test agents receive.

```rust
/// The full contract for a TDD node — behavioral spec composed with execution context.
/// Produced after ContractValidating passes. Both the impl and test agents receive this.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TddContractArtifact {
    pub spec: BehavioralSpec,
    pub context: ExecutionContext,
}
```

Convenience accessors (add as inherent methods):

```rust
impl TddContractArtifact {
    pub fn impl_targets(&self) -> &[String]  { &self.context.impl_targets }
    pub fn test_path(&self)     -> &str       { &self.context.test_path }
    pub fn validate_command(&self) -> &str    { &self.context.validate_command }
    pub fn test_framework(&self)   -> &str    { &self.context.test_framework }
}
```

### TddRepairDecision + TddDiagnosis (unchanged from v1 TDD-4)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TddDiagnosis {
    ImplBug, TestBug, ImplTestMismatch, ContractAmbiguity, Impossible, Environmental,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TddRepairDecision {
    pub diagnosis: TddDiagnosis,
    pub fix_impl: bool,
    pub fix_test: bool,
    pub refine_contract: bool,
    pub escalate_to_human: bool,
    #[serde(default)] pub impl_context: Option<String>,
    #[serde(default)] pub test_context: Option<String>,
    #[serde(default)] pub contract_refinement: Option<String>,
    #[serde(default)] pub escalation_reason: Option<String>,
}
```

### OrchaNodeKind::Tdd (unchanged from v1 TDD-1)

```rust
pub enum OrchaNodeKind {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Review { prompt: String },
    Plan { task: String },
    Tdd {
        task: String,
        #[serde(default)] contract_prompt: Option<String>,
        #[serde(default)] max_repair_cycles: Option<u8>,
        #[serde(default)] max_spec_cycles: Option<u8>,
    },
}
```

`max_spec_cycles` is new in v2 — maps to `MaxSpecCycles` in `DispatchTdd.tla`.

Also update `OrchaNodeSpec::Tdd` to add `max_spec_cycles: Option<u8>`.

### Exports

Add to the `pub use` / public exports at the top of the file:
`BehavioralSpec`, `ExecutionContext`, `SpecReview`, `TddContractArtifact`,
`TddRepairDecision`, `TddDiagnosis`.

## Changes to src/activations/orcha/graph_runtime.rs

Add `add_tdd` builder to `OrchaGraph`:

```rust
pub async fn add_tdd(
    &self,
    task: impl Into<String>,
    contract_prompt: Option<String>,
    max_repair_cycles: Option<u8>,
    max_spec_cycles: Option<u8>,
) -> Result<String, String> {
    let kind = OrchaNodeKind::Tdd {
        task: task.into(),
        contract_prompt,
        max_repair_cycles,
        max_spec_cycles,
    };
    self.add_spec(NodeSpec::Task {
        data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
        handle: None,
    })
    .await
}
```

Update `build_child_graph` to handle `OrchaNodeSpec::Tdd`:

```rust
OrchaNodeSpec::Tdd { task, contract_prompt, max_repair_cycles, max_spec_cycles } =>
    graph.add_tdd(task, contract_prompt, max_repair_cycles, max_spec_cycles).await,
```

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-2: Storage — orcha_tdd_contracts and orcha_tdd_behavioral_specs [agent]

blocked_by: [TDD-1]

Add storage for the two phases of ContractValidating to
`src/activations/orcha/pm/storage.rs` and expose methods on `Pm`.

## Changes to src/activations/orcha/pm/storage.rs

### Table definitions

In `PmStorage::new` (or wherever table creation happens), add:

```sql
-- Full composed contract (BehavioralSpec + ExecutionContext).
-- Saved at end of ContractValidating. Used by impl, test, and repair agents.
CREATE TABLE IF NOT EXISTS orcha_tdd_contracts (
    id          TEXT PRIMARY KEY,
    graph_id    TEXT NOT NULL,
    node_id     TEXT NOT NULL,
    contract    TEXT NOT NULL,   -- JSON: TddContractArtifact
    spec_cycle  INTEGER NOT NULL DEFAULT 0,
    repair_cycle INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tdd_contracts
    ON orcha_tdd_contracts(graph_id, node_id, spec_cycle);

-- Intermediate BehavioralSpec — saved immediately after dispatch_contract.
-- Enables dispatch_spec_review to access the spec without rerunning the agent.
CREATE TABLE IF NOT EXISTS orcha_tdd_behavioral_specs (
    id          TEXT PRIMARY KEY,
    graph_id    TEXT NOT NULL,
    node_id     TEXT NOT NULL,
    spec        TEXT NOT NULL,   -- JSON: BehavioralSpec
    spec_cycle  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tdd_behavioral_specs
    ON orcha_tdd_behavioral_specs(graph_id, node_id, spec_cycle);
```

### PmStorage methods

```rust
/// Save a BehavioralSpec immediately after dispatch_contract produces it.
pub async fn save_tdd_behavioral_spec(
    &self,
    graph_id: &str,
    node_id: &str,
    spec_cycle: u32,
    spec: &crate::activations::orcha::types::BehavioralSpec,
) -> Result<(), String>

/// Retrieve the BehavioralSpec for a given spec cycle.
pub async fn get_tdd_behavioral_spec(
    &self,
    graph_id: &str,
    node_id: &str,
    spec_cycle: u32,
) -> Result<Option<crate::activations::orcha::types::BehavioralSpec>, String>

/// Save the composed TddContractArtifact after ContractValidating passes.
pub async fn save_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
    spec_cycle: u32,
    repair_cycle: u32,
    contract: &crate::activations::orcha::types::TddContractArtifact,
) -> Result<(), String>

/// Retrieve the latest contract (highest spec_cycle + repair_cycle).
pub async fn get_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
) -> Result<Option<crate::activations::orcha::types::TddContractArtifact>, String>
```

Primary key pattern: `"{graph_id}:{node_id}:{spec_cycle}"` for specs;
`"{graph_id}:{node_id}:{spec_cycle}:{repair_cycle}"` for contracts.

### Pm pass-through methods

Add the four methods above as pass-throughs on `Pm` in
`src/activations/orcha/pm/activation.rs`.

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-3: ContractPhase — dispatch_contract → BehavioralSpec [agent]

blocked_by: [TDD-2]

Add `dispatch_contract` and `extract_json_block` helper to
`src/activations/orcha/graph_runner.rs`.

Maps to TLA+ `ContractPhase`: spec agent runs, produces a contract, transitions
to `ContractValidating`.

## Helper: extract_json_block (unchanged from v1 TDD-3)

```rust
fn extract_json_block(text: &str) -> Option<&str> {
    let start = text.find("```json")? + 7;
    let rest = &text[start..];
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}
```

## dispatch_contract

Runs a single `dispatch_task` call. Outputs `BehavioralSpec` only — no file
paths, no shell commands. Saves result via `pm.save_tdd_behavioral_spec`.

```rust
async fn dispatch_contract<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    task: &str,
    contract_prompt_override: Option<&str>,
    node_id: &str,
    graph_id: &str,
    model: Model,
    working_directory: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
    spec_cycle: u32,
    /// Issues from previous spec review — appended when refining a failed spec.
    prior_issues: &[String],
) -> Result<BehavioralSpec, String>
```

### Spec agent system prompt

```rust
const CONTRACT_SYSTEM: &str = r#"
You are a software specification agent. Your job is to define a precise
BEHAVIORAL CONTRACT for the following task.

Output ONLY a ```json block containing a BehavioralSpec with these fields:
- preconditions: list of strings — what must hold before the call
- postconditions: list of strings — what must hold after the call
- properties: list of invariants suitable for property-based tests
  (e.g. "∀ valid inputs: f is idempotent", "result length equals input length")
- examples: list of {"input": ..., "expected": ...} objects — concrete unit test cases
- edge_cases: list of boundary conditions that must be explicitly handled
- out_of_scope: list of behaviors explicitly excluded from this spec

DO NOT include file paths, test commands, or any implementation details.
A separate project analysis agent will derive those independently.
Be precise enough that an impl agent and a test agent can work independently
and produce compatible, verifiable code.
"#;
```

When `prior_issues` is non-empty (spec refinement cycle), append:

```rust
let refinement_note = if prior_issues.is_empty() {
    String::new()
} else {
    format!(
        "\n\nPREVIOUS SPEC REVIEW ISSUES (address these in your revised spec):\n{}",
        prior_issues.join("\n- ")
    )
};

let full_prompt = format!(
    "{}{}\n\nTASK:\n{}",
    contract_prompt_override.unwrap_or(CONTRACT_SYSTEM),
    refinement_note,
    task,
);
```

After `dispatch_task` returns, extract JSON → parse `BehavioralSpec` →
call `pm.save_tdd_behavioral_spec(graph_id, node_id, spec_cycle, &spec)`.

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-4: ContractValidating — dispatch_spec_review + dispatch_execution_context [agent]

blocked_by: [TDD-3]

Add `dispatch_spec_review` and `dispatch_execution_context` to
`src/activations/orcha/graph_runner.rs`. Both run in parallel during
`ContractValidating`. On spec review failure, `dispatch_tdd` increments
`spec_cycle` and reruns `dispatch_contract`. On pass, compose and save
`TddContractArtifact`.

Maps to TLA+: `SpecPass` → `Branching`; `SpecFail` → `ContractPhase`;
`SpecExhausted` → `Failed`.

## dispatch_spec_review

A `dispatch_task` call that checks `BehavioralSpec` against a structured
checklist. Returns `SpecReview`.

```rust
async fn dispatch_spec_review<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    spec: &BehavioralSpec,
    node_id: &str,
    graph_id: &str,
    model: Model,
    working_directory: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<SpecReview, String>
```

### Spec review prompt

```rust
const SPEC_REVIEW_SYSTEM: &str = r#"
You are a behavioral spec reviewer. Check the following BehavioralSpec for
internal consistency. Do NOT evaluate whether the spec is easy to implement.
Only check logical coherence.

Checklist (check ALL of these):
1. Do any postconditions contradict any preconditions?
2. Do the examples violate the postconditions or properties?
3. Are any properties vacuously true (true for all inputs regardless of behavior)?
4. Do the examples cover at least one case from each stated edge_case?
5. Are any out_of_scope items actually implied by the properties?

Output ONLY a ```json block with these fields:
- consistent: boolean — true if no checklist items failed
- issues: list of strings — one per failed checklist item (empty if consistent)
- suggestions: list of strings — non-blocking improvements (may be empty)
- method: "semantic_review"
"#;
```

After `dispatch_task`, extract JSON → parse `SpecReview`. The `method` field
is hardcoded in the spec review prompt and serves as an observability tag for
future TLC-based replacement (same interface, different `method` value).

## dispatch_execution_context

A `dispatch_task` call that reads project structure and produces `ExecutionContext`.
Runs concurrently with `dispatch_spec_review` (both launched via `tokio::join!`).

```rust
async fn dispatch_execution_context<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    spec: &BehavioralSpec,
    task: &str,
    node_id: &str,
    graph_id: &str,
    model: Model,
    working_directory: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<ExecutionContext, String>
```

### Execution context agent prompt

```rust
const EXEC_CONTEXT_SYSTEM: &str = r#"
You are a project analysis agent. Given a task description and a behavioral spec,
determine the execution context for this project.

Use your tools to explore the project structure (list files, read existing tests,
check Cargo.toml / package.json / pyproject.toml, etc.) and then output ONLY a
```json block with these fields:
- impl_targets: list of source file paths the impl agent should modify
- test_path: the exact path where the test agent should write its test file
  (follow existing project conventions, e.g. tests/integration/ or src/lib_test.rs)
- validate_command: the shell command that runs only these tests (exit 0 = pass)
  (e.g. "cargo test --package foo -- my_module 2>&1")
- test_framework: the test framework in use, e.g. "cargo test", "pytest", "jest"
  (this drives property-based test library selection: proptest/hypothesis/fast-check)

Be precise. These values are passed directly to the impl and test agents.
The validate_command must be a real, runnable command in this project.
"#;

let prompt = format!(
    "{}\n\nTASK:\n{}\n\nBEHAVIORAL SPEC (for reference only — do not repeat it):\n```json\n{}\n```",
    EXEC_CONTEXT_SYSTEM,
    task,
    serde_json::to_string_pretty(spec).unwrap_or_default(),
);
```

## ContractValidating phase in dispatch_tdd

In the `dispatch_tdd` skeleton (added in TDD-5), `ContractValidating` looks like:

```rust
// Run spec review and execution context derivation in parallel.
let (review_result, context_result) = tokio::join!(
    dispatch_spec_review(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &spec, node_id, graph_id, model, working_directory,
        output_tx.clone(), cancel_rx.clone(),
        ticket_id.as_deref().map(|t| format!("{t}-review")),
    ),
    dispatch_execution_context(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &spec, task, node_id, graph_id, model, working_directory,
        output_tx.clone(), cancel_rx.clone(),
        ticket_id.as_deref().map(|t| format!("{t}-ctx")),
    ),
);

let review = review_result?;
let exec_context = context_result?;

// SpecFail: refine contract (if spec_cycle allows).
if !review.consistent {
    if spec_cycle >= max_spec_cycles {
        return Err(format!(
            "TDD spec exhausted {} spec cycle(s). Last issues:\n{}",
            max_spec_cycles, review.issues.join("\n")
        ));
    }
    // Return (spec, review) to the outer loop to retry ContractPhase.
    return Err(/* signal retry — handled by dispatch_tdd loop, not a hard error */);
}

// SpecPass: compose and save.
let contract = TddContractArtifact { spec, context: exec_context };
pm.save_tdd_contract(graph_id, node_id, spec_cycle, 0, &contract).await
    .map_err(|e| format!("failed to persist contract: {e}"))?;
```

The actual control flow (spec_cycle loop) lives in `dispatch_tdd` (TDD-5).
`dispatch_spec_review` and `dispatch_execution_context` are pure functions
that return their results; `dispatch_tdd` owns the loop logic.

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-5: Branching + Validating — dispatch_tdd_branches and dispatch_tdd skeleton [agent]

blocked_by: [TDD-4]

Add `dispatch_tdd_branches` and the main `dispatch_tdd` function (phases 1–3,
phase 4 stubbed) to `src/activations/orcha/graph_runner.rs`.

Maps to TLA+ `Branching` → `Validating` → (`Complete` | `ValidateFail`).

## dispatch_tdd_branches

Dispatches impl and test agents in parallel using the full `TddContractArtifact`.
Then runs `dispatch_validate`. Returns `(impl_text, test_text, Option<failure_log>)`.

```rust
async fn dispatch_tdd_branches<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    contract: &TddContractArtifact,
    original_task: &str,
    node_id: &str,
    graph_id: &str,
    model: Model,
    working_directory: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
    repair_context: Option<&str>,
    impl_extra: Option<&str>,
    test_extra: Option<&str>,
) -> Result<(String, String, Option<String>), String>
```

### Agent prompts

Both agents receive the full `TddContractArtifact`. The behavioral spec provides
the "what"; the execution context provides "where" and "how to validate".

```rust
let contract_block = format!(
    "CONTRACT (do not deviate from this spec):\n```json\n{}\n```\n\nORIGINAL TASK:\n{}\n",
    serde_json::to_string_pretty(contract).unwrap_or_default(),
    original_task,
);

let impl_prompt = format!(
    "{contract_block}\n\
     You are the IMPLEMENTATION agent. Write the implementation only.\n\
     Modify these files: {targets}\n\
     Do NOT write tests. The test agent works independently from the same spec.\n\
     Use {framework} conventions for the project.\n\
     {repair}\n{extra}",
    targets   = contract.impl_targets().join(", "),
    framework = contract.test_framework(),
    repair    = repair_context.unwrap_or(""),
    extra     = impl_extra.unwrap_or(""),
);

let test_prompt = format!(
    "{contract_block}\n\
     You are the TEST agent. Write tests only.\n\
     Write your test file to: {test_path}\n\
     Do NOT modify implementation files. The impl agent works independently.\n\
     The validate command is: {cmd}\n\
     For each property in the spec, write at least one property-based test \
     using {framework}.\n\
     {repair}\n{extra}",
    test_path = contract.test_path(),
    cmd       = contract.validate_command(),
    framework = contract.test_framework(),
    repair    = repair_context.unwrap_or(""),
    extra     = test_extra.unwrap_or(""),
);
```

The property-based test instruction (`For each property...`) drives `hypothesis`
for pytest, `proptest` for cargo, `fast-check` for jest/vitest — the test agent
selects the correct library based on `test_framework`.

Run in parallel via `tokio::join!`, then `dispatch_validate`. Signature and
return pattern are identical to v1 TDD-3.

## dispatch_tdd — main loop

```rust
async fn dispatch_tdd<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    arbor: Arc<ArborStorage>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    graph: &OrchaGraph,
    task: String,
    contract_prompt: Option<String>,
    max_repair_cycles: u8,
    max_spec_cycles: u8,
    node_id: &str,
    model: Model,
    working_directory: String,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<Option<NodeOutput>, String>
```

### Spec loop (ContractPhase → ContractValidating)

```rust
let mut prior_issues: Vec<String> = Vec::new();
let contract = 'spec: {
    for spec_cycle in 0..=max_spec_cycles {
        // ContractPhase
        let spec = dispatch_contract(
            claudecode.clone(), loopback_storage.clone(), pm.clone(),
            &task, contract_prompt.as_deref(), node_id, &graph.graph_id,
            model, &working_directory, output_tx.clone(), cancel_rx.clone(),
            ticket_id.clone(), spec_cycle, &prior_issues,
        ).await?;

        // ContractValidating (parallel)
        let (review_result, context_result) = tokio::join!(
            dispatch_spec_review(/* ... */),
            dispatch_execution_context(/* ... */),
        );

        let review = review_result?;
        let exec_context = context_result?;

        if review.consistent {
            let contract = TddContractArtifact { spec, context: exec_context };
            pm.save_tdd_contract(&graph.graph_id, node_id, spec_cycle, 0, &contract).await?;
            break 'spec contract;
        }

        // SpecFail: collect issues for next spec cycle
        prior_issues = review.issues;

        if spec_cycle >= max_spec_cycles as u32 {
            return Err(format!(
                "TDD spec exhausted {} spec cycle(s). Last issues:\n{}",
                max_spec_cycles,
                prior_issues.join("\n"),
            ));
        }
    }
    unreachable!()
};
```

### Branch + repair loop (Branching → Validating → Repairing)

After `contract` is established:

```rust
// Phase 2+3: branches + validate
let (impl_text, test_text, failure) = dispatch_tdd_branches(
    claudecode.clone(), loopback_storage.clone(), pm.clone(),
    &contract, &task, node_id, &graph.graph_id, model, &working_directory,
    output_tx.clone(), cancel_rx.clone(), ticket_id.clone(),
    None, None, None,
).await?;

if failure.is_none() {
    return Ok(Some(NodeOutput::Single(Token::ok_data(
        serde_json::json!({ "contract": contract, "impl": impl_text, "test": test_text }),
    ))));
}

// Phase 4: repair (stubbed — TDD-6 implements this)
Err(format!("TDD validation failed (repair not yet implemented): {}",
    failure.unwrap_or_default()))
```

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-6: Repairing + EscalatingToHuman — dispatch_tdd_repair and repair loop [agent]

blocked_by: [TDD-5]

Add `dispatch_tdd_repair` and replace the stub at the end of `dispatch_tdd`
with the full repair loop. Maps to TLA+ `Repairing` → (`RepairRebranch` |
`RepairRefineContract` | `RepairEscalate` | `RepairAmbiguityExhausted`) →
`EscalatingToHuman`.

## dispatch_tdd_repair

```rust
async fn dispatch_tdd_repair<P: HubContext + 'static>(
    claudecode: Arc<ClaudeCode<P>>,
    loopback_storage: Arc<LoopbackStorage>,
    pm: Arc<Pm>,
    contract: &TddContractArtifact,
    original_task: &str,
    impl_text: &str,
    test_text: &str,
    failure_log: &str,
    node_id: &str,
    graph_id: &str,
    model: Model,
    working_directory: &str,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<TddRepairDecision, String>
```

### Repair agent prompt (unchanged from v1 TDD-4)

Receives the full `TddContractArtifact` (so it can reason about both spec and
execution context). Diagnoses root cause. Returns `TddRepairDecision` JSON.

```rust
const REPAIR_SYSTEM: &str = r#"
You are a TDD repair agent. A contract-driven development cycle has failed.
Diagnose the root cause and decide the minimum repair.

- impl_bug: contract and tests agree; impl is wrong. Set fix_impl=true.
- test_bug: impl is correct per contract; tests have wrong expectations. Set fix_test=true.
- impl_test_mismatch: both diverged from contract. Set fix_impl=true and fix_test=true.
- contract_ambiguity: spec didn't pin down a behavior. Set refine_contract=true.
- impossible: the contract cannot be implemented. Set escalate_to_human=true.
- environmental: missing files, wrong path. Set fix_impl=true, explain in impl_context.

Output ONLY a ```json block matching TddRepairDecision.
"#;
```

## Full repair loop in dispatch_tdd

Replace the stub with:

```rust
let mut contract = contract;
let mut impl_text = impl_text;
let mut test_text = test_text;
let mut failure_log = failure.unwrap();
let mut current_spec_cycle = /* spec_cycle from above spec loop */;

for repair_cycle in 1..=max_repair_cycles {
    let decision = dispatch_tdd_repair(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &contract, &task, &impl_text, &test_text, &failure_log,
        node_id, &graph.graph_id, model, &working_directory,
        output_tx.clone(), cancel_rx.clone(), ticket_id.clone(),
    ).await?;

    // EscalatingToHuman: Impossible or ContractAmbiguity exhausted
    if decision.escalate_to_human {
        let prompt = format!(
            "TDD cycle failed and cannot be automatically repaired.\n\n\
             Task: {}\nDiagnosis: {:?}\nReason: {}\n\nFailure:\n{}\n\n\
             Approve to retry with human-provided guidance, or deny to fail the node.",
            task,
            decision.diagnosis,
            decision.escalation_reason.as_deref().unwrap_or("(none)"),
            &failure_log[..failure_log.len().min(1000)],
        );
        dispatch_review(
            loopback_storage.clone(),
            &graph.graph_id,
            prompt,
            output_tx.clone(),
            cancel_rx.clone(),
        ).await?;
        // Human approved — fall through to rebranch with generic retry context.
    }

    // ContractAmbiguity: refine spec (RepairRefineContract in TLA+)
    if decision.refine_contract {
        let refinement_issues = vec![
            decision.contract_refinement
                .unwrap_or_else(|| "Please clarify the ambiguous behavior.".to_string())
        ];
        current_spec_cycle += 1;
        if current_spec_cycle > max_spec_cycles as u32 {
            return Err(format!(
                "TDD spec exhausted during repair at repair_cycle={}.",
                repair_cycle
            ));
        }
        let new_spec = dispatch_contract(
            claudecode.clone(), loopback_storage.clone(), pm.clone(),
            &task, contract_prompt.as_deref(), node_id, &graph.graph_id,
            model, &working_directory, output_tx.clone(), cancel_rx.clone(),
            ticket_id.clone(), current_spec_cycle, &refinement_issues,
        ).await?;
        // Re-run ContractValidating to validate the refined spec.
        let (review_result, context_result) = tokio::join!(
            dispatch_spec_review(/* new_spec */),
            dispatch_execution_context(/* new_spec, task */),
        );
        let review = review_result?;
        let exec_context = context_result?;
        if !review.consistent {
            return Err(format!(
                "Refined spec failed review at repair_cycle={}:\n{}",
                repair_cycle, review.issues.join("\n")
            ));
        }
        contract = TddContractArtifact { spec: new_spec, context: exec_context };
        pm.save_tdd_contract(&graph.graph_id, node_id, current_spec_cycle, repair_cycle, &contract).await?;
    }

    // Rebranch (RepairRebranch in TLA+)
    let (new_impl, new_test, new_failure) = dispatch_tdd_branches(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &contract, &task, node_id, &graph.graph_id, model, &working_directory,
        output_tx.clone(), cancel_rx.clone(), ticket_id.clone(),
        None,
        if decision.fix_impl { decision.impl_context.as_deref() } else { None },
        if decision.fix_test { decision.test_context.as_deref() } else { None },
    ).await?;

    impl_text = new_impl;
    test_text = new_test;

    match new_failure {
        None => {
            return Ok(Some(NodeOutput::Single(Token::ok_data(
                serde_json::json!({
                    "contract": contract,
                    "impl": impl_text,
                    "test": test_text,
                    "repair_cycles": repair_cycle,
                    "spec_cycles": current_spec_cycle,
                }),
            ))));
        }
        Some(log) => { failure_log = log; }
    }
}

Err(format!(
    "TDD node exhausted {} repair cycle(s). Last failure:\n{}",
    max_repair_cycles, failure_log
))
```

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


---

# TDD-7: Wire — dispatch_node, ticket compiler, integration validation [agent]

blocked_by: [TDD-6]

Wire `OrchaNodeKind::Tdd` into `dispatch_node` in `graph_runner.rs`, add
`[agent/tdd]` support to `ticket_compiler.rs`, and run a full `cargo build`
to confirm end-to-end compilation.

## Changes to dispatch_node in graph_runner.rs

```rust
OrchaNodeKind::Tdd { task, contract_prompt, max_repair_cycles, max_spec_cycles } => {
    dispatch_tdd(
        claudecode,
        arbor,
        loopback_storage,
        pm,
        graph,
        task,
        contract_prompt,
        max_repair_cycles.unwrap_or(2),
        max_spec_cycles.unwrap_or(2),
        node_id,
        model,
        working_directory,
        output_tx,
        cancel_rx,
        ticket_id,
    ).await
}
```

## Exhaustiveness check

Search `graph_runner.rs` and `activation.rs` for all `match` arms on
`OrchaNodeKind` or `OrchaNodeSpec`. Add `Tdd { .. } => { /* not dispatched here */ }`
wildcard arms where needed. Do not add Tdd handling where it doesn't belong.

## Changes to ticket_compiler.rs

In `build_graph`, add an `"agent/tdd"` arm:

```rust
"agent/tdd" => {
    let task = t.task.clone().ok_or_else(|| {
        format!("Ticket '{}' [agent/tdd] has no body text", t.id)
    })?;
    OrchaNodeSpec::Tdd {
        task,
        contract_prompt: None,
        max_repair_cycles: None,
        max_spec_cycles: None,
    }
}
```

## Imports

Ensure the following are imported at the top of `graph_runner.rs` from
`super::types::`:
`BehavioralSpec`, `ExecutionContext`, `SpecReview`, `TddContractArtifact`,
`TddRepairDecision`, `TddDiagnosis`.

## Validation

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0
