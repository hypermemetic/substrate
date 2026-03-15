# TDD Node Implementation Plan

This document introduces `[agent/tdd]` as a first-class ticket type and its
runtime counterpart `OrchaNodeKind::Tdd`. A TDD node encapsulates a four-phase
contract-first development loop: spec → parallel impl+test → validate → repair.

The entire loop runs inside `dispatch_tdd` in graph_runner.rs. The parent graph
sees a single node. All internal structure is managed by the dispatcher.

---

# TDD-1: Add TddContractArtifact type, OrchaNodeKind::Tdd, and compiler tag [agent]

Add the `TddContractArtifact` struct, the `Tdd` variant to `OrchaNodeKind` and
`OrchaNodeSpec`, the `add_tdd` builder on `OrchaGraph`, and `[agent/tdd]` support
in `ticket_compiler.rs`.

## Changes to src/activations/orcha/types.rs

Add `TddContractArtifact` — the structured output from the contract phase,
consumed by both the impl agent and the test agent:

```rust
/// The structured contract produced by the spec phase of a TDD node.
/// Drives both the impl branch and the test branch, and specifies
/// exactly where tests live and how to validate them.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TddContractArtifact {
    /// Source files the impl agent should modify.
    pub impl_targets: Vec<String>,
    /// Path where the test agent must write its test file.
    /// Project-specific, e.g. "tests/integration/tdd_run_plan.rs"
    pub test_path: String,
    /// Shell command that validates the implementation against the tests.
    /// e.g. "cargo test --package plexus-substrate -- tdd_run_plan 2>&1"
    pub validate_command: String,
    /// What must be true before calling the function/module.
    pub preconditions: Vec<String>,
    /// What must be true after calling the function/module.
    pub postconditions: Vec<String>,
    /// Invariants that hold across all inputs (good candidates for property tests).
    pub properties: Vec<String>,
    /// Concrete input/expected-output pairs for unit tests.
    pub examples: Vec<serde_json::Value>,
    /// Boundary and edge cases that must be explicitly handled.
    pub edge_cases: Vec<String>,
    /// Behaviors explicitly excluded from this spec (prevents scope creep).
    pub out_of_scope: Vec<String>,
}
```

Add the `Tdd` variant to `OrchaNodeKind`:

```rust
pub enum OrchaNodeKind {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Review { prompt: String },
    Plan { task: String },
    Tdd {
        /// The task description passed to the contract agent.
        task: String,
        /// Optional override for the contract agent's system prompt.
        #[serde(default)]
        contract_prompt: Option<String>,
        /// How many repair cycles before escalating to human (default: 2).
        #[serde(default)]
        max_repair_cycles: Option<u8>,
    },
}
```

Add the `Tdd` variant to `OrchaNodeSpec`:

```rust
pub enum OrchaNodeSpec {
    Task { task: String, #[serde(default)] max_retries: Option<u8> },
    Synthesize { task: String, #[serde(default)] max_retries: Option<u8> },
    Validate { command: String, cwd: Option<String>, #[serde(default)] max_retries: Option<u8> },
    Gather { strategy: GatherStrategy },
    Review { prompt: String },
    Plan { task: String },
    Tdd {
        task: String,
        #[serde(default)] contract_prompt: Option<String>,
        #[serde(default)] max_repair_cycles: Option<u8>,
    },
}
```

Also add `TddContractArtifact` to the pub exports at the top of the file (alongside
`OrchaEvent`, `OrchaNodeSpec`, etc.) and add `use schemars::JsonSchema;` if not
already imported.

## Changes to src/activations/orcha/graph_runtime.rs

Add `add_tdd` builder to `OrchaGraph`:

```rust
/// Add a TDD node — runs spec → parallel impl+test → validate → repair loop.
pub async fn add_tdd(
    &self,
    task: impl Into<String>,
    contract_prompt: Option<String>,
    max_repair_cycles: Option<u8>,
) -> Result<String, String> {
    let kind = OrchaNodeKind::Tdd {
        task: task.into(),
        contract_prompt,
        max_repair_cycles,
    };
    self.add_spec(NodeSpec::Task {
        data: serde_json::to_value(&kind).map_err(|e| e.to_string())?,
        handle: None,
    })
    .await
}
```

Also update `build_child_graph` to handle `OrchaNodeSpec::Tdd`:

```rust
OrchaNodeSpec::Tdd { task, contract_prompt, max_repair_cycles } =>
    graph.add_tdd(task, contract_prompt, max_repair_cycles).await,
```

## Changes to src/activations/orcha/ticket_compiler.rs

In `build_graph`, add an `"agent/tdd"` arm:

```rust
"agent/tdd" => {
    let task = t.task.clone().ok_or_else(|| {
        format!("Ticket '{}' [agent/tdd] has no body text", t.id)
    })?;
    OrchaNodeSpec::Tdd { task, contract_prompt: None, max_repair_cycles: None }
}
```

The body text becomes the task. `max_repair_cycles` and `contract_prompt` are
not exposed in the ticket file format for now — defaults are used.

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


# TDD-2: Add TDD contract storage to pm [agent]

blocked_by: [TDD-1]

Add `orcha_tdd_contracts` table to `src/activations/orcha/pm/storage.rs` and
expose `save_tdd_contract` / `get_tdd_contract` on the `Pm` struct.

## Changes to src/activations/orcha/pm/storage.rs

In the `PmStorage::new` (or wherever table creation happens), add:

```sql
CREATE TABLE IF NOT EXISTS orcha_tdd_contracts (
    id         TEXT PRIMARY KEY,
    graph_id   TEXT NOT NULL,
    node_id    TEXT NOT NULL,
    contract   TEXT NOT NULL,
    cycle      INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tdd_contracts
    ON orcha_tdd_contracts(graph_id, node_id, cycle);
```

Add to `PmStorage`:

```rust
/// Persist a TDD contract artifact for a given node + repair cycle.
/// cycle=0 is the initial contract; cycle>0 are refinements.
pub async fn save_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
    cycle: u32,
    contract: &crate::activations::orcha::types::TddContractArtifact,
) -> Result<(), String> {
    let id = format!("{}:{}:{}", graph_id, node_id, cycle);
    let json = serde_json::to_string(contract).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "INSERT OR REPLACE INTO orcha_tdd_contracts (id, graph_id, node_id, contract, cycle, created_at)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&id).bind(graph_id).bind(node_id)
    .bind(&json).bind(cycle as i64).bind(now)
    .execute(&self.pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Retrieve the latest contract for a node (highest cycle number).
pub async fn get_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
) -> Result<Option<crate::activations::orcha::types::TddContractArtifact>, String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT contract FROM orcha_tdd_contracts
         WHERE graph_id = ? AND node_id = ?
         ORDER BY cycle DESC LIMIT 1"
    )
    .bind(graph_id).bind(node_id)
    .fetch_optional(&self.pool)
    .await
    .map_err(|e| e.to_string())?;

    match row {
        None => Ok(None),
        Some((json,)) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| e.to_string()),
    }
}
```

## Changes to src/activations/orcha/pm/activation.rs (the Pm wrapper)

Add pass-through methods on `Pm`:

```rust
pub async fn save_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
    cycle: u32,
    contract: &crate::activations::orcha::types::TddContractArtifact,
) -> Result<(), String> {
    self.storage.save_tdd_contract(graph_id, node_id, cycle, contract).await
}

pub async fn get_tdd_contract(
    &self,
    graph_id: &str,
    node_id: &str,
) -> Result<Option<crate::activations::orcha::types::TddContractArtifact>, String> {
    self.storage.get_tdd_contract(graph_id, node_id).await
}
```

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


# TDD-3: Implement dispatch_tdd phases 1–3 (contract, branches, validate) [agent]

blocked_by: [TDD-2]

Add `dispatch_contract`, `extract_json_block`, and the first three phases of
`dispatch_tdd` to `src/activations/orcha/graph_runner.rs`.

## Helper: extract_json_block

```rust
/// Extract the first ```json ... ``` code block from Claude's text output.
fn extract_json_block(text: &str) -> Option<&str> {
    let start = text.find("```json")? + 7;
    let rest = &text[start..];
    // skip optional newline after the fence
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("```")?;
    Some(rest[..end].trim())
}
```

## Phase 1: dispatch_contract

Runs a single Task agent with a structured prompt that instructs Claude to output
a `TddContractArtifact` JSON block. Parses and persists the artifact.

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
    cycle: u32,
) -> Result<TddContractArtifact, String>
```

The prompt sent to Claude (prepended to `task`):

```rust
const CONTRACT_SYSTEM: &str = r#"
You are a software specification agent. Your job is to define a precise contract
for the following task such that an IMPLEMENTATION agent and a TEST agent can work
independently and produce compatible code.

Output ONLY a ```json block containing a TddContractArtifact with these fields:
- impl_targets: list of source file paths to modify
- test_path: exactly where the test file should be written (project-appropriate path)
- validate_command: the shell command that runs just these tests (exit 0 = pass)
- preconditions: list of strings — what must hold before the call
- postconditions: list of strings — what must hold after the call
- properties: list of invariants suitable for property-based tests
- examples: list of {"input": ..., "expected": ...} objects
- edge_cases: list of boundary conditions that must be handled
- out_of_scope: list of behaviors explicitly excluded

Be precise enough that impl and test can be written without further communication.
The validate_command must be a real command that will work in this project.
"#;

let full_prompt = format!("{}\n\nTASK:\n{}",
    contract_prompt_override.unwrap_or(CONTRACT_SYSTEM),
    task
);
```

After dispatch_task returns, extract and parse the JSON:

```rust
let text = match &result {
    Some(NodeOutput::Single(token)) => {
        match token.payload.as_ref() {
            Some(TokenPayload::Data { value }) => {
                value.get("text").and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .ok_or("contract agent produced no text")?
            }
            _ => return Err("contract agent produced non-data output".to_string()),
        }
    }
    _ => return Err("contract agent produced no output".to_string()),
};

let json_str = extract_json_block(&text)
    .ok_or("contract agent output contained no ```json block")?;

let artifact: TddContractArtifact = serde_json::from_str(json_str)
    .map_err(|e| format!("contract JSON parse error: {e}\nRaw: {json_str}"))?;

pm.save_tdd_contract(graph_id, node_id, cycle, &artifact).await
    .map_err(|e| format!("failed to persist contract: {e}"))?;

Ok(artifact)
```

## Phase 2+3: dispatch_tdd_branches

Dispatches impl and test agents in parallel, then validates.

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
    /// Extra context appended to both prompts (used by repair cycles).
    repair_context: Option<&str>,
    /// Extra context for impl only (e.g. "tests expect X").
    impl_extra: Option<&str>,
    /// Extra context for test only (e.g. "impl returns Y").
    test_extra: Option<&str>,
) -> Result<(String, String, Option<String>), String>
// Returns (impl_output_text, test_output_text, validate_failure_log)
```

Build prompts by prepending the contract JSON to each branch:

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
     Do NOT write tests. The test agent works independently.\n\
     {repair}\n{extra}",
    contract_block = contract_block,
    targets = contract.impl_targets.join(", "),
    repair = repair_context.unwrap_or(""),
    extra = impl_extra.unwrap_or(""),
);

let test_prompt = format!(
    "{contract_block}\n\
     You are the TEST agent. Write tests only.\n\
     Write your test file to: {test_path}\n\
     Do NOT modify implementation files. The impl agent works independently.\n\
     The validate command is: {cmd}\n\
     {repair}\n{extra}",
    contract_block = contract_block,
    test_path = contract.test_path,
    cmd = contract.validate_command,
    repair = repair_context.unwrap_or(""),
    extra = test_extra.unwrap_or(""),
);
```

Run in parallel:

```rust
let (impl_result, test_result) = tokio::join!(
    dispatch_task(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        impl_prompt, vec![], node_id, model, working_directory.to_string(),
        graph_id, output_tx.clone(), cancel_rx.clone(),
        ticket_id.as_deref().map(|t| format!("{t}-impl")),
    ),
    dispatch_task(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        test_prompt, vec![], node_id, model, working_directory.to_string(),
        graph_id, output_tx.clone(), cancel_rx.clone(),
        ticket_id.as_deref().map(|t| format!("{t}-test")),
    ),
);

let impl_text = impl_result?
    .as_ref().and_then(|o| output_text(o))
    .unwrap_or_default();
let test_text = test_result?
    .as_ref().and_then(|o| output_text(o))
    .unwrap_or_default();
```

Run validate:

```rust
let validate_result = dispatch_validate(
    contract.validate_command.clone(),
    Some(working_directory.to_string()),
).await;

let failure_log = match validate_result {
    Ok(()) => return Ok((impl_text, test_text, None)),  // success
    Err(log) => log,
};

Ok((impl_text, test_text, Some(failure_log)))
```

## Skeleton dispatch_tdd (phases 1-3 only, phase 4 stubbed)

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
    node_id: &str,
    model: Model,
    working_directory: String,
    output_tx: tokio::sync::mpsc::UnboundedSender<OrchaEvent>,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    ticket_id: Option<String>,
) -> Result<Option<NodeOutput>, String> {
    // Phase 1: contract
    let contract = dispatch_contract(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &task, contract_prompt.as_deref(), node_id, &graph.graph_id,
        model, &working_directory, output_tx.clone(), cancel_rx.clone(),
        ticket_id.clone(), 0,
    ).await?;

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

    // Phase 4: repair (stubbed — TDD-4 implements this)
    Err(format!("TDD validation failed (repair not yet implemented): {}",
        failure.unwrap_or_default()))
}
```

## Note on dispatch_validate signature

`dispatch_validate` currently takes `command: String, cwd: Option<String>` and
returns `Result<(), String>` where Err carries the failure output.
Verify the actual signature in graph_runner.rs around line 749 and adjust the
call accordingly.

## Validation

validate: cargo check --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0


# TDD-4: Implement dispatch_tdd phase 4 — repair agent and escalation [agent]

blocked_by: [TDD-3]

Add the `TddRepairDecision` type, `dispatch_tdd_repair` function, and complete
the repair loop in `dispatch_tdd` in `src/activations/orcha/graph_runner.rs`.

## TddRepairDecision type

Add to `src/activations/orcha/types.rs`:

```rust
/// Structured decision output from the TDD repair agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TddDiagnosis {
    /// Implementation is wrong; re-run impl with error context.
    ImplBug,
    /// Tests are wrong; re-run test with error context.
    TestBug,
    /// Both diverged from contract; re-run both.
    ImplTestMismatch,
    /// Contract is ambiguous; refine spec before re-branching.
    ContractAmbiguity,
    /// Spec is impossible or requires architectural changes.
    Impossible,
    /// Failure is environmental (missing deps, wrong path, etc.).
    Environmental,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TddRepairDecision {
    pub diagnosis: TddDiagnosis,
    pub fix_impl: bool,
    pub fix_test: bool,
    pub refine_contract: bool,
    pub escalate_to_human: bool,
    /// Context appended to impl prompt on re-run.
    #[serde(default)]
    pub impl_context: Option<String>,
    /// Context appended to test prompt on re-run.
    #[serde(default)]
    pub test_context: Option<String>,
    /// If refine_contract: description of what to change in the spec.
    #[serde(default)]
    pub contract_refinement: Option<String>,
    /// If escalate_to_human: human-readable explanation of why.
    #[serde(default)]
    pub escalation_reason: Option<String>,
}
```

## dispatch_tdd_repair

Runs a repair agent that reads the full failure context and classifies it:

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

The repair agent prompt:

```rust
const REPAIR_SYSTEM: &str = r#"
You are a TDD repair agent. A contract-driven development cycle has failed.
Diagnose the root cause and decide the minimum repair.

Failure cases and their meanings:
- impl_bug: contract and tests agree; impl is wrong. Set fix_impl=true.
- test_bug: impl is correct per contract; tests have wrong expectations. Set fix_test=true.
- impl_test_mismatch: both diverged from contract in incompatible ways. Set fix_impl=true and fix_test=true.
- contract_ambiguity: neither impl nor test is wrong — the spec didn't pin down a behavior. Set refine_contract=true.
- impossible: the contract specifies something that cannot be implemented in this codebase. Set escalate_to_human=true.
- environmental: failure is due to missing files, wrong paths, missing dependencies. Set fix_impl=true with impl_context explaining the env issue.

Output ONLY a ```json block matching the TddRepairDecision schema.
Be specific in impl_context and test_context — they are appended directly to the agent prompts.
"#;

let prompt = format!(
    "{}\n\nORIGINAL TASK:\n{}\n\nCONTRACT:\n```json\n{}\n```\n\nIMPLEMENTATION OUTPUT:\n{}\n\nTEST CODE:\n{}\n\nVALIDATION FAILURE:\n{}",
    REPAIR_SYSTEM,
    original_task,
    serde_json::to_string_pretty(contract).unwrap_or_default(),
    &impl_text[..impl_text.len().min(3000)],
    &test_text[..test_text.len().min(3000)],
    &failure_log[..failure_log.len().min(2000)],
);
```

After running dispatch_task, extract and parse the JSON using `extract_json_block`.

## Complete repair loop in dispatch_tdd

Replace the stub at the end of `dispatch_tdd` with:

```rust
let mut contract = contract;
let mut impl_text = impl_text;
let mut test_text = test_text;
let mut failure_log = failure.unwrap();

for cycle in 1..=max_repair_cycles {
    // Classify the failure.
    let decision = dispatch_tdd_repair(
        claudecode.clone(), loopback_storage.clone(), pm.clone(),
        &contract, &task, &impl_text, &test_text, &failure_log,
        node_id, &graph.graph_id, model, &working_directory,
        output_tx.clone(), cancel_rx.clone(), ticket_id.clone(),
    ).await?;

    // Human escalation: reuse dispatch_review's loopback approval pattern.
    if decision.escalate_to_human {
        let prompt = format!(
            "TDD cycle failed and cannot be automatically repaired.\n\n\
             Task: {}\n\nDiagnosis: {:?}\n\nReason: {}\n\nFailure:\n{}\n\n\
             Approve to retry with human-provided guidance, or deny to fail the node.",
            task,
            decision.diagnosis,
            decision.escalation_reason.as_deref().unwrap_or("(none)"),
            &failure_log[..failure_log.len().min(1000)],
        );
        // dispatch_review handles the full poll-until-resolved loop
        dispatch_review(
            loopback_storage.clone(),
            &graph.graph_id,
            prompt,
            output_tx.clone(),
            cancel_rx.clone(),
        ).await?;
        // If review approved, fall through and try one more repair cycle
        // with the human's response as context (approval message is not yet
        // threaded — acceptable for initial impl; use a generic retry context).
    }

    // Contract refinement: re-run contract agent with refinement guidance.
    if decision.refine_contract {
        let refinement_task = format!(
            "{}\n\nCONTRACT REFINEMENT NEEDED:\n{}",
            task,
            decision.contract_refinement.as_deref().unwrap_or("Please clarify the ambiguous parts."),
        );
        contract = dispatch_contract(
            claudecode.clone(), loopback_storage.clone(), pm.clone(),
            &refinement_task, None, node_id, &graph.graph_id,
            model, &working_directory, output_tx.clone(), cancel_rx.clone(),
            ticket_id.clone(), cycle,
        ).await?;
    }

    // Re-run branches based on decision.
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
                    "cycles": cycle,
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


# TDD-5: Wire dispatch_tdd into dispatch_node [agent]

blocked_by: [TDD-4]

Add the `OrchaNodeKind::Tdd` arm to `dispatch_node` in
`src/activations/orcha/graph_runner.rs` and fix any exhaustiveness warnings
from the new variant.

## Changes to dispatch_node

Add the Tdd arm:

```rust
OrchaNodeKind::Tdd { task, contract_prompt, max_repair_cycles } => {
    dispatch_tdd(
        claudecode,
        arbor,
        loopback_storage,
        pm,
        graph,
        task,
        contract_prompt,
        max_repair_cycles.unwrap_or(2),
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

Search for all `match kind {` or `match spec {` blocks in graph_runner.rs and
activation.rs that pattern-match on `OrchaNodeKind` or `OrchaNodeSpec`. Add a
`Tdd { .. } => { /* not matched here */ }` wildcard arm to any that don't
already handle it, or a proper arm if the context requires it.

## Imports

Ensure `TddContractArtifact`, `TddRepairDecision`, `TddDiagnosis` are imported
or referenced with their full paths at the top of graph_runner.rs. They live in
`super::types::`.

## Validation

validate: cargo build --package plexus-substrate --features mcp-gateway 2>&1 | grep "^error" | head -5 && exit 1 || exit 0
