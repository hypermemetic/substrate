# ORCHA-2: Task Decomposition Metrics & Subagent Measurement

## Overview

Orcha's core value proposition is intelligent task decomposition - breaking complex work into manageable subtasks and delegating to specialized agents. However, we currently have no visibility into:

1. **How often** Orcha spawns subagents
2. **How well** it decomposes tasks (quality of subtask definitions)
3. **Whether** it parallelizes appropriately
4. **Agent hierarchy** structure (depth, breadth, balance)

This plan adds measurement and testing infrastructure to evaluate Orcha's decomposition behavior without changing the core orchestration logic.

## Problem Statement

**Current State:**
- Orcha has `spawn_agent` capability
- Multi-agent mode exists
- No metrics on agent spawning patterns
- No way to evaluate if tasks are appropriately decomposed
- Can't measure if Orcha prefers single-agent vs multi-agent approaches

**Questions We Can't Answer:**
- Does Orcha actively break down complex tasks?
- What percentage of tasks spawn subagents?
- What's the typical agent hierarchy shape?
- Are subagents given focused, manageable subtasks?
- Does it parallelize independent work?
- How does model choice (Sonnet vs Opus) affect decomposition?

## Goals

1. **Add metrics collection** - Track agent spawning, hierarchy, timing
2. **Create test scenarios** - Tasks designed to measure decomposition propensity
3. **Build analysis tools** - Scripts to evaluate decomposition quality
4. **Establish baselines** - Understand current behavior before optimization
5. **No behavior changes** - Pure measurement, no orchestrator modifications

## Non-Goals

- Changing Orcha's orchestration logic
- Optimizing decomposition behavior (future work)
- Real-time dashboards or monitoring UI
- Production metrics infrastructure

---

## Tickets

### ORCHA-2.1: Add Agent Hierarchy Metrics

**Priority:** High
**Estimate:** 2 hours
**Dependencies:** None

**Description:**

Add structured metrics to track agent spawning patterns and hierarchy structure.

**Changes:**

1. Add metrics types in `src/activations/orcha/types.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub session_id: String,
    pub total_agents: u32,
    pub hierarchy_depth: u32,
    pub hierarchy_breadth: u32, // max siblings at any level
    pub spawn_timeline: Vec<AgentSpawnEvent>,
    pub parallel_agents: u32, // max concurrent agents
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpawnEvent {
    pub agent_id: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub timestamp: String,
    pub task_description: String,
    pub model: String,
}
```

2. Add method to retrieve metrics in `src/activations/orcha/activation.rs`:
```rust
/// Get agent hierarchy metrics for a session
///
/// Returns statistics about agent spawning patterns,
/// hierarchy structure, and task decomposition.
#[plexus_macros::hub_method]
async fn get_agent_metrics(
    &self,
    request: GetAgentMetricsRequest, // { session_id: String }
) -> impl Stream<Item = AgentMetrics> + Send + 'static {
    let storage = self.storage.clone();
    let session_id = request.session_id;

    stream! {
        // Get all agents for session
        let agents = storage.list_agents(&session_id).await.unwrap_or_default();

        if agents.is_empty() {
            yield AgentMetrics {
                session_id: session_id.clone(),
                total_agents: 0,
                hierarchy_depth: 0,
                hierarchy_breadth: 0,
                spawn_timeline: vec![],
                parallel_agents: 0,
            };
            return;
        }

        // Calculate hierarchy depth (max distance from root)
        let hierarchy_depth = calculate_max_depth(&agents);

        // Calculate hierarchy breadth (max siblings at any level)
        let hierarchy_breadth = calculate_max_breadth(&agents);

        // Build spawn timeline
        let mut spawn_timeline = Vec::new();
        for agent in &agents {
            spawn_timeline.push(AgentSpawnEvent {
                agent_id: agent.agent_id.clone(),
                parent_id: agent.parent_agent_id.clone(),
                depth: calculate_agent_depth(agent, &agents),
                timestamp: agent.created_at.to_rfc3339(),
                task_description: extract_task_from_agent(agent),
                model: agent.model.clone(),
            });
        }
        spawn_timeline.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Calculate max parallel agents (overlapping time ranges)
        let parallel_agents = calculate_max_parallel(&agents);

        yield AgentMetrics {
            session_id,
            total_agents: agents.len() as u32,
            hierarchy_depth,
            hierarchy_breadth,
            spawn_timeline,
            parallel_agents,
        };
    }
}
```

3. Add helper functions in `src/activations/orcha/metrics.rs` (new file):
```rust
pub fn calculate_max_depth(agents: &[AgentInfo]) -> u32 { ... }
pub fn calculate_max_breadth(agents: &[AgentInfo]) -> u32 { ... }
pub fn calculate_agent_depth(agent: &AgentInfo, all_agents: &[AgentInfo]) -> u32 { ... }
pub fn calculate_max_parallel(agents: &[AgentInfo]) -> u32 { ... }
pub fn extract_task_from_agent(agent: &AgentInfo) -> String { ... }
```

**Testing:**
- Session with 1 agent → depth=0, breadth=1
- Session with 3 sequential agents → depth=3, breadth=1
- Session with 1 root + 3 children → depth=1, breadth=3
- Verify parallel calculation with overlapping time ranges

**Success Criteria:**
- Can retrieve hierarchy metrics via API
- Metrics accurately reflect agent structure
- Performance acceptable for 100+ agent sessions

---

### ORCHA-2.2: Create Decomposition Test Scenarios

**Priority:** High
**Estimate:** 3 hours
**Dependencies:** ORCHA-2.1

**Description:**

Design test tasks that measure Orcha's propensity to decompose work into subagents.

**Test Scenarios:**

1. **Simple Task (No Decomposition Expected)**
```bash
# Task: Single-file analysis
synapse substrate orcha run_task \
  --request.task "Read README.md and count the number of sections" \
  --request.model sonnet

# Expected: 1 agent (no spawning needed)
# Measure: total_agents = 1, hierarchy_depth = 0
```

2. **Multi-File Analysis (Optional Decomposition)**
```bash
# Task: Analysis across multiple files
synapse substrate orcha run_task \
  --request.task "Analyze all Rust files in src/activations/orcha/ and summarize the purpose of each module" \
  --request.model sonnet

# Expected: 1-5 agents (could parallelize file analysis)
# Measure: Does it spawn per-file agents or handle sequentially?
```

3. **Multi-Step Task (Sequential Decomposition Expected)**
```bash
# Task: Research → Analysis → Report
synapse substrate orcha run_task \
  --request.task "1) Find all TODO comments in the codebase, 2) Categorize them by urgency, 3) Generate a markdown report" \
  --request.model sonnet

# Expected: 2-3 agents (one per step)
# Measure: hierarchy_depth ≥ 1, sequential spawning
```

4. **Parallel Work (Parallel Decomposition Expected)**
```bash
# Task: Independent subtasks
synapse substrate orcha run_task \
  --request.task "Compare performance: 1) Benchmark cache hit rate, 2) Benchmark IR generation speed, 3) Benchmark hash computation. Run all benchmarks in parallel." \
  --request.model sonnet

# Expected: 3+ agents (parallel execution)
# Measure: parallel_agents ≥ 3, hierarchy_breadth ≥ 3
```

5. **Complex Multi-Layer Task (Deep Decomposition Expected)**
```bash
# Task: Plan → Implement → Test → Document
synapse substrate orcha run_task \
  --request.task "Add a new cache eviction policy: 1) Research existing policies, 2) Design the new policy, 3) Implement it, 4) Add tests, 5) Update documentation" \
  --request.model opus

# Expected: 5-10 agents (nested decomposition)
# Measure: hierarchy_depth ≥ 2, total_agents ≥ 5
```

6. **Deliberately Overwhelming Task (Decomposition Required)**
```bash
# Task: Intentionally large scope
synapse substrate orcha run_task \
  --request.task "Perform a complete security audit of the codebase: check for SQL injection, XSS, command injection, path traversal, secrets in code, insecure dependencies, and generate a detailed report with remediation steps for each finding" \
  --request.model opus

# Expected: 7+ agents (one per vulnerability class + aggregation)
# Measure: Strong decomposition signal
```

**Test Matrix:**

| Scenario | Task Complexity | Expected Agents | Expected Depth | Expected Breadth | Parallelization |
|----------|----------------|-----------------|----------------|------------------|-----------------|
| 1. Simple | Low | 1 | 0 | 1 | No |
| 2. Multi-file | Medium | 1-5 | 0-1 | 1-5 | Maybe |
| 3. Multi-step | Medium | 2-3 | 1-2 | 1 | No |
| 4. Parallel | Medium | 3-4 | 0-1 | 3-4 | Yes |
| 5. Complex | High | 5-10 | 2-3 | 2-3 | Partial |
| 6. Overwhelming | Very High | 7+ | 2-4 | 3-5 | Yes |

**Success Criteria:**
- Can run all 6 scenarios successfully
- Metrics collected for each scenario
- Clear decomposition patterns emerge

---

### ORCHA-2.3: Build Decomposition Analysis Tool

**Priority:** Medium
**Estimate:** 2 hours
**Dependencies:** ORCHA-2.1, ORCHA-2.2

**Description:**

Create script to analyze decomposition metrics and generate comparative reports.

**Tool: `scripts/analyze-decomposition.sh`**

```bash
#!/usr/bin/env bash
# Analyze Orcha task decomposition behavior

SESSION_ID=$1

if [ -z "$SESSION_ID" ]; then
    echo "Usage: $0 <session_id>"
    exit 1
fi

# Fetch metrics
METRICS=$(synapse substrate orcha get_agent_metrics \
    --request.session_id "$SESSION_ID" \
    --raw)

# Parse and display
echo "=== Agent Hierarchy Metrics ==="
echo "$METRICS" | jq '{
    total_agents,
    hierarchy_depth,
    hierarchy_breadth,
    parallel_agents
}'

echo ""
echo "=== Spawn Timeline ==="
echo "$METRICS" | jq -r '.spawn_timeline[] |
    "\(.timestamp | sub("\\.[0-9]+Z$"; "Z") | fromdate | strftime("%H:%M:%S")) |
    Depth \(.depth) | \(.agent_id) | \(.task_description[:60])..."'

echo ""
echo "=== Decomposition Score ==="
# Simple scoring heuristic
TOTAL=$(echo "$METRICS" | jq '.total_agents')
DEPTH=$(echo "$METRICS" | jq '.hierarchy_depth')
BREADTH=$(echo "$METRICS" | jq '.hierarchy_breadth')
PARALLEL=$(echo "$METRICS" | jq '.parallel_agents')

if [ "$TOTAL" -eq 1 ]; then
    SCORE="NONE (single agent)"
elif [ "$DEPTH" -eq 0 ] && [ "$TOTAL" -gt 1 ]; then
    SCORE="FLAT (parallel only)"
elif [ "$DEPTH" -gt 0 ] && [ "$BREADTH" -eq 1 ]; then
    SCORE="SEQUENTIAL (linear chain)"
elif [ "$DEPTH" -gt 1 ] && [ "$BREADTH" -gt 2 ]; then
    SCORE="HIERARCHICAL (strong decomposition)"
else
    SCORE="PARTIAL (some decomposition)"
fi

echo "Decomposition Pattern: $SCORE"
echo "  Total Agents: $TOTAL"
echo "  Max Depth: $DEPTH"
echo "  Max Breadth: $BREADTH"
echo "  Max Parallel: $PARALLEL"
```

**Tool: `scripts/compare-decomposition.sh`**

```bash
#!/usr/bin/env bash
# Compare decomposition across multiple runs

echo "=== Orcha Decomposition Comparison ==="
echo ""
printf "%-40s | Agents | Depth | Breadth | Parallel | Pattern\n" "Session ID"
printf "%s\n" "$(printf '%.0s-' {1..100})"

for SESSION_ID in "$@"; do
    METRICS=$(synapse substrate orcha get_agent_metrics \
        --request.session_id "$SESSION_ID" \
        --raw 2>/dev/null)

    if [ -z "$METRICS" ]; then
        echo "Session $SESSION_ID not found"
        continue
    fi

    TOTAL=$(echo "$METRICS" | jq -r '.total_agents')
    DEPTH=$(echo "$METRICS" | jq -r '.hierarchy_depth')
    BREADTH=$(echo "$METRICS" | jq -r '.hierarchy_breadth')
    PARALLEL=$(echo "$METRICS" | jq -r '.parallel_agents')

    # Determine pattern
    if [ "$TOTAL" -eq 1 ]; then
        PATTERN="Single"
    elif [ "$DEPTH" -gt 1 ] && [ "$BREADTH" -gt 2 ]; then
        PATTERN="Hierarchical"
    elif [ "$PARALLEL" -gt 2 ]; then
        PATTERN="Parallel"
    else
        PATTERN="Sequential"
    fi

    printf "%-40s | %6s | %5s | %7s | %8s | %s\n" \
        "$SESSION_ID" "$TOTAL" "$DEPTH" "$BREADTH" "$PARALLEL" "$PATTERN"
done
```

**Usage:**
```bash
# Analyze single session
./scripts/analyze-decomposition.sh orcha-abc-123

# Compare multiple runs
./scripts/compare-decomposition.sh \
    orcha-simple-task-001 \
    orcha-complex-task-002 \
    orcha-parallel-task-003
```

**Success Criteria:**
- Scripts work with actual Orcha sessions
- Clear visualization of decomposition patterns
- Easy comparison across scenarios

---

### ORCHA-2.4: Run Baseline Decomposition Tests

**Priority:** High
**Estimate:** 3 hours
**Dependencies:** ORCHA-2.1, ORCHA-2.2, ORCHA-2.3

**Description:**

Execute all test scenarios and collect baseline metrics for current Orcha behavior.

**Process:**

1. **Run all 6 test scenarios** from ORCHA-2.2
2. **Collect session IDs** and metrics for each
3. **Analyze decomposition patterns** using scripts from ORCHA-2.3
4. **Document findings** in `ORCHA_BASELINE_METRICS.md`

**Data to Collect:**

For each scenario:
- Session ID
- Total execution time
- Agent count, depth, breadth, parallelization
- Spawn timeline visualization
- Task descriptions given to subagents
- Success/failure of decomposition

**Analysis Questions:**

1. **Does Orcha decompose complex tasks?**
   - What percentage of scenarios spawned subagents?
   - At what complexity threshold does spawning occur?

2. **Quality of decomposition**
   - Are subagent tasks focused and manageable?
   - Do task descriptions make sense?
   - Are dependencies handled correctly?

3. **Parallelization behavior**
   - Does it recognize independent work?
   - Are parallel tasks truly concurrent?

4. **Model differences**
   - Does Sonnet decompose differently than Opus?
   - Is decomposition quality model-dependent?

5. **Hierarchy structure**
   - Flat (breadth) vs deep (depth) decomposition?
   - Does it match task structure?

**Document Format: `ORCHA_BASELINE_METRICS.md`**

```markdown
# Orcha Decomposition Baseline Metrics

**Date:** 2026-03-03
**Orcha Version:** 1.0.0
**Test Environment:** Substrate localhost:4444

## Executive Summary

- **Decomposition Rate:** X% of tasks spawned subagents
- **Average Agents per Task:** N agents (σ=M)
- **Typical Pattern:** [Single/Sequential/Parallel/Hierarchical]
- **Parallelization:** X% of independent work parallelized

## Scenario Results

### Scenario 1: Simple Task
- Session ID: orcha-xxx-001
- Agents: 1
- Hierarchy: Depth=0, Breadth=1
- Pattern: ✅ Single agent (expected)
- Notes: No decomposition needed

[... repeat for all 6 scenarios ...]

## Observations

### Decomposition Propensity
- Simple tasks: No decomposition (correct)
- Multi-step tasks: [Y/N] spawned sequential agents
- Parallel tasks: [Y/N] spawned parallel agents
- Complex tasks: [Y/N] created hierarchy

### Task Quality
- Subagent tasks were [clear/vague/appropriate]
- Examples of good decomposition: [...]
- Examples of poor decomposition: [...]

### Bottlenecks
- [Any patterns where decomposition should happen but didn't]
- [Any unnecessary spawning]

## Recommendations

1. [Areas for improvement]
2. [Scenarios to add for better testing]
3. [Metrics to track going forward]
```

**Success Criteria:**
- All 6 scenarios executed successfully
- Metrics collected and documented
- Clear baseline established for future comparison

---

### ORCHA-2.5: Add Decomposition Visualization

**Priority:** Low
**Estimate:** 2 hours
**Dependencies:** ORCHA-2.1, ORCHA-2.3

**Description:**

Create ASCII tree visualization of agent hierarchy for easy debugging.

**Tool: `scripts/visualize-agents.sh`**

```bash
#!/usr/bin/env bash
# Visualize agent hierarchy as ASCII tree

SESSION_ID=$1

# Fetch agents
AGENTS=$(synapse substrate orcha list_agents \
    --request.session_id "$SESSION_ID" \
    --raw)

echo "=== Agent Hierarchy for $SESSION_ID ==="
echo ""

# Build tree structure (Python helper)
python3 <<EOF
import json
import sys

agents_json = '''$AGENTS'''
agents = json.loads(agents_json)

# Build parent-child map
children = {}
for agent in agents:
    parent = agent.get('parent_agent_id')
    if parent not in children:
        children[parent] = []
    children[parent].append(agent)

# Find root (no parent)
roots = [a for a in agents if not a.get('parent_agent_id')]

def print_tree(agent, prefix="", is_last=True):
    # Print current agent
    connector = "└── " if is_last else "├── "
    status_icon = {
        'completed': '✓',
        'running': '▶',
        'blocked': '⏸',
        'failed': '✗',
        'pending': '○'
    }.get(agent.get('status', 'unknown'), '?')

    task_desc = agent.get('task_description', 'N/A')[:50]
    print(f"{prefix}{connector}{status_icon} {agent['agent_id']} | {task_desc}")

    # Print children
    agent_children = children.get(agent['agent_id'], [])
    for i, child in enumerate(agent_children):
        is_last_child = (i == len(agent_children) - 1)
        extension = "    " if is_last else "│   "
        print_tree(child, prefix + extension, is_last_child)

# Print all roots
for i, root in enumerate(roots):
    print_tree(root, "", i == len(roots) - 1)
EOF
```

**Output Example:**
```
=== Agent Hierarchy for orcha-complex-task-002 ===

✓ agent-root-001 | Perform complete security audit of codebase
├── ✓ agent-child-001 | Check for SQL injection vulnerabilities
├── ▶ agent-child-002 | Check for XSS vulnerabilities
│   ├── ✓ agent-grandchild-001 | Scan TypeScript files for XSS
│   └── ⏸ agent-grandchild-002 | Scan generated code for XSS
├── ○ agent-child-003 | Check for command injection
└── ○ agent-child-004 | Aggregate findings and generate report
```

**Success Criteria:**
- Clear visual representation of hierarchy
- Shows agent status at a glance
- Works with deep/wide hierarchies

---

### ORCHA-2.6: Document Measurement Methodology

**Priority:** Medium
**Estimate:** 1 hour
**Dependencies:** All above tickets

**Description:**

Document how to use decomposition metrics for ongoing evaluation.

**Create: `docs/ORCHA_DECOMPOSITION_TESTING.md`**

```markdown
# Orcha Decomposition Testing Guide

## Overview

This guide explains how to measure and evaluate Orcha's task decomposition behavior.

## Quick Start

```bash
# 1. Run a task
synapse substrate orcha run_task \
  --request.task "Your complex task here" \
  --request.model sonnet
# Note the session_id from output

# 2. Get metrics
synapse substrate orcha get_agent_metrics \
  --request.session_id "orcha-xxx-xxx"

# 3. Visualize hierarchy
./scripts/visualize-agents.sh orcha-xxx-xxx

# 4. Analyze decomposition
./scripts/analyze-decomposition.sh orcha-xxx-xxx
```

## Metrics Explained

### Agent Count
Total number of agents spawned (including root).
- **1 agent**: No decomposition
- **2-3 agents**: Light decomposition
- **4+ agents**: Strong decomposition

### Hierarchy Depth
Maximum distance from root to leaf agent.
- **0**: Single agent or flat parallelization
- **1-2**: Shallow hierarchy (common)
- **3+**: Deep hierarchy (rare, indicates nested delegation)

### Hierarchy Breadth
Maximum number of sibling agents at any level.
- **1**: Sequential execution only
- **2-3**: Some parallelization
- **4+**: Heavy parallelization

### Parallel Agents
Maximum number of agents running concurrently.
- Indicates actual parallel execution
- Compare to breadth (planned vs actual parallelism)

## Interpreting Results

### Good Decomposition Patterns

**Sequential for dependent work:**
```
Task: "Read file A, then process results, then write file B"
Pattern: 3 agents, depth=2, breadth=1 ✓
```

**Parallel for independent work:**
```
Task: "Analyze modules A, B, C independently"
Pattern: 4 agents, depth=0, breadth=3 ✓
```

**Hierarchical for complex tasks:**
```
Task: "Security audit: check 5 vulnerability types, aggregate report"
Pattern: 7 agents, depth=2, breadth=5 ✓
```

### Anti-Patterns

**No decomposition on complex task:**
```
Task: "Complete codebase refactoring"
Pattern: 1 agent, depth=0 ✗
→ Should spawn agents for different modules
```

**Excessive decomposition on simple task:**
```
Task: "Count lines in a file"
Pattern: 5 agents, depth=3 ✗
→ Unnecessary overhead
```

## Test Scenarios

See ORCHA-2.2 for standard test scenarios covering:
1. Simple tasks (no decomposition expected)
2. Multi-file tasks (optional decomposition)
3. Sequential tasks (depth expected)
4. Parallel tasks (breadth expected)
5. Complex tasks (hierarchy expected)
6. Overwhelming tasks (strong decomposition required)

## Comparing Models

Test decomposition differences between models:

```bash
# Sonnet
synapse substrate orcha run_task \
  --request.task "..." \
  --request.model sonnet

# Opus
synapse substrate orcha run_task \
  --request.task "..." \
  --request.model opus

# Compare
./scripts/compare-decomposition.sh orcha-sonnet-xxx orcha-opus-yyy
```

## Continuous Monitoring

Recommended practices:
1. Run standard test scenarios monthly
2. Track decomposition metrics over time
3. Alert on significant changes (regression detection)
4. Review agent task descriptions for quality

## Future Work

- Automatic decomposition scoring
- ML-based pattern recognition
- Optimization recommendations
- A/B testing different prompts/strategies
```

**Success Criteria:**
- Clear documentation for measurement process
- Examples and anti-patterns included
- Actionable guidance for interpretation

---

## Implementation Order

1. **ORCHA-2.1** - Add metrics collection (enables all other work)
2. **ORCHA-2.2** - Create test scenarios (defines what to measure)
3. **ORCHA-2.3** - Build analysis tools (makes metrics actionable)
4. **ORCHA-2.4** - Run baseline tests (establish current behavior)
5. **ORCHA-2.5** - Add visualization (debugging aid)
6. **ORCHA-2.6** - Document methodology (knowledge sharing)

## Success Metrics

- **Baseline Established**: Current decomposition behavior documented
- **Measurable**: Can quantify decomposition propensity
- **Comparable**: Can compare scenarios and models
- **Actionable**: Metrics inform optimization decisions
- **Non-Invasive**: No changes to orchestration logic

## Future Optimization (Out of Scope)

Based on baseline findings, potential improvements:
- **Prompt engineering** - Encourage decomposition in system prompts
- **Heuristics** - Rules for when to suggest spawning agents
- **Task analysis** - Parse task complexity before execution
- **Auto-parallelization** - Detect independent subtasks automatically
- **Agent templates** - Predefined decomposition patterns for common task types

## Open Questions

1. **What is the "right" amount of decomposition?**
   - Too little: Agent overwhelmed by complexity
   - Too much: Coordination overhead dominates
   - Likely task-dependent

2. **How do we measure decomposition quality?**
   - Count isn't enough - need to assess appropriateness
   - Subagent task descriptions should be evaluated
   - Success rate of subtasks matters

3. **Model sensitivity:**
   - Do different models decompose differently?
   - Should we tune decomposition by model?

4. **Temporal dynamics:**
   - Does decomposition change over conversation length?
   - Do agents learn to decompose better over time?
