#!/usr/bin/env python3
"""
Orcha Chaos Monkey Test
=======================
Launches a fleet of concurrent orcha graphs, then unleashes chaos:
  - Random node failure injection
  - Claude process killing
  - Substrate hard-crash and recovery observation

Usage:
  python3 chaos_test.py [--no-crash] [--rounds N]

Requires: substrate running on ws://127.0.0.1:4444
          synapse binary at ~/.local/bin/synapse
"""

import subprocess
import json
import time
import random
import argparse
import threading
import sys
import os
from dataclasses import dataclass, field
from typing import Optional

SYNAPSE = os.path.expanduser("~/.local/bin/synapse")
SUBSTRATE_DIR = os.path.dirname(os.path.abspath(__file__))
LANG_ENV = {**os.environ, "LANG": "C.UTF-8"}


# ─── Synapse helpers ──────────────────────────────────────────────────────────

def _unwrap(raw: dict) -> dict:
    """Unwrap the -j (JSON mode) envelope: {"type":"data","content":{...}} → content."""
    if raw.get("type") == "data" and "content" in raw:
        return raw["content"]
    return raw


def synapse(*args, timeout=30):
    """Run a synapse command and return parsed JSON events as a list."""
    cmd = [SYNAPSE, "-j", "substrate"] + list(args)
    try:
        result = subprocess.run(
            cmd, capture_output=True, text=True,
            timeout=timeout, env=LANG_ENV
        )
        events = []
        for line in result.stdout.strip().splitlines():
            line = line.strip()
            if line:
                try:
                    events.append(_unwrap(json.loads(line)))
                except json.JSONDecodeError:
                    pass
        return events
    except subprocess.TimeoutExpired:
        return [{"type": "error", "message": "timeout"}]
    except Exception as e:
        return [{"type": "error", "message": str(e)}]


def synapse_stream(args, on_event, stop_event=None, timeout=300):
    """Run a streaming synapse command, calling on_event for each JSON line."""
    cmd = [SYNAPSE, "-j", "substrate"] + args
    try:
        proc = subprocess.Popen(
            cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, env=LANG_ENV
        )
        for line in proc.stdout:
            if stop_event and stop_event.is_set():
                proc.terminate()
                break
            line = line.strip()
            if line:
                try:
                    event = _unwrap(json.loads(line))
                    if not on_event(event):
                        proc.terminate()
                        break
                except json.JSONDecodeError:
                    pass
        proc.wait(timeout=5)
    except Exception as e:
        on_event({"type": "error", "message": str(e)})


def log(tag, msg, color=None):
    colors = {"red": "\033[91m", "green": "\033[92m", "yellow": "\033[93m",
              "blue": "\033[94m", "cyan": "\033[96m", "reset": "\033[0m"}
    c = colors.get(color, "")
    r = colors["reset"]
    print(f"  [{tag}] {c}{msg}{r}", flush=True)


# ─── Graph launchers ─────────────────────────────────────────────────────────

GRAPHS = [
    {
        "name": "fibonacci",
        "desc": "Write + test Python fibonacci (linear chain with validate)",
        "task": (
            "Write a Python file /tmp/chaos_fib.py with a function fib(n) that returns "
            "the nth Fibonacci number recursively with memoization. "
            "Then write /tmp/chaos_fib_test.py with pytest tests (at least 4 tests: "
            "fib(0), fib(1), fib(10), fib(20)). "
            "Run tests with: python -m pytest /tmp/chaos_fib_test.py -v. "
            "Structure: implement → write tests → validate."
        ),
    },
    {
        "name": "parallel-sort",
        "desc": "Two sort algorithms in parallel, then benchmark comparison",
        "task": (
            "Build a Python sorting benchmark at /tmp/chaos_sort/. "
            "Two tasks in PARALLEL: "
            "(1) write merge_sort.py with a merge sort implementation, "
            "(2) write quick_sort.py with a quicksort implementation. "
            "Then a third task that writes benchmark.py that imports both, "
            "runs each on a list of 10000 random numbers 5 times, and prints timing. "
            "Finally validate with: cd /tmp/chaos_sort && python benchmark.py. "
            "The two sort implementations MUST be written in parallel graph nodes."
        ),
    },
    {
        "name": "json-pipeline",
        "desc": "JSON transform pipeline with error handling",
        "task": (
            "Build a Python JSON transformation pipeline at /tmp/chaos_json/. "
            "Three tasks: "
            "(1) write transformer.py with functions: flatten_dict(d) that flattens nested dicts "
            "with dot notation keys, and filter_nulls(d) that removes None values recursively. "
            "(2) write sample_data.json with a deeply nested JSON object (at least 3 levels). "
            "(3) write pipeline.py that loads sample_data.json, applies flatten_dict then "
            "filter_nulls, and prints the result. "
            "Validate with: cd /tmp/chaos_json && python pipeline.py. "
            "Tasks (1) and (2) run in PARALLEL, task (3) depends on both."
        ),
    },
    {
        "name": "retry-victim",
        "desc": "Task designed to test retry: writes a counter file and fails until attempt 3",
        "task": (
            "Write a Python script /tmp/chaos_retry.py that: "
            "1. Reads an integer from /tmp/chaos_retry_count.txt (or 0 if missing). "
            "2. Increments it and writes it back. "
            "3. If the count is less than 3, exit with sys.exit(1) and print 'attempt N, need 3'. "
            "4. If count >= 3, print 'SUCCESS after N attempts' and exit 0. "
            "Then run: python /tmp/chaos_retry.py. "
            "Note: the test EXPECTS the first runs to fail — use max_retries=3 on the validate node."
        ),
    },
    {
        "name": "string-utils",
        "desc": "String utility library with synthesize node summarizing the work",
        "task": (
            "Build a Python string utilities library at /tmp/chaos_str/. "
            "Three PARALLEL implementation tasks: "
            "(1) write slugify.py with slugify(text) that lowercases, removes special chars, "
            "replaces spaces with hyphens. "
            "(2) write truncate.py with truncate(text, max_len, ellipsis='...') that truncates "
            "at word boundaries. "
            "(3) write camel.py with to_camel_case(text) and to_snake_case(text). "
            "Then a test task that writes tests/test_utils.py testing all three modules, "
            "followed by validate: cd /tmp/chaos_str && python -m pytest tests/ -v. "
            "Finally a SYNTHESIZE node that summarizes what was built and any interesting "
            "design decisions made during implementation."
        ),
    },
]


@dataclass
class GraphRun:
    name: str
    graph_id: Optional[str] = None
    status: str = "launching"
    events: list = field(default_factory=list)
    thread: Optional[threading.Thread] = None
    error: Optional[str] = None


def launch_graph(run: GraphRun, task: str):
    """Launch a run_plan in a background thread, tracking events."""
    def _run():
        run.status = "running"
        last_type = None

        def on_event(event):
            run.events.append(event)
            t = event.get("type", "")
            if t == "node_started":
                label = event.get("label") or event.get("ticket_id", "")
                log(run.name, f"node_started: {label}", "cyan")
            elif t == "node_complete":
                label = event.get("label") or event.get("ticket_id", "")
                pct = event.get("percentage", "")
                log(run.name, f"node_complete: {label} ({pct}%)", "green")
            elif t == "node_failed":
                label = event.get("label") or event.get("ticket_id", "")
                err = event.get("error", "")[:80]
                log(run.name, f"node_FAILED: {label} — {err}", "red")
            elif t == "complete":
                run.status = "complete"
                run.graph_id = event.get("session_id", "")
                log(run.name, f"COMPLETE (graph: {run.graph_id})", "green")
                return False  # stop streaming
            elif t == "failed":
                run.status = "failed"
                run.error = event.get("error", "")
                log(run.name, f"FAILED: {run.error}", "red")
                return False
            return True

        synapse_stream(
            ["orcha", "run_plan", "--task", task, "--model", "sonnet",
             "--working-directory", "/tmp"],
            on_event=on_event,
            timeout=600,
        )
        if run.status == "running":
            run.status = "timeout"
            log(run.name, "timed out", "red")

    run.thread = threading.Thread(target=_run, name=run.name, daemon=True)
    run.thread.start()


# ─── Chaos primitives ────────────────────────────────────────────────────────

def list_running_nodes():
    events = synapse("chaos", "list_running_nodes", timeout=10)
    nodes = []
    for e in events:
        if e.get("type") == "node":
            nodes.append(e)
    return nodes


def inject_failure(graph_id, node_id, error="chaos: injected failure"):
    events = synapse("chaos", "inject_failure",
                     "--graph-id", graph_id,
                     "--node-id", node_id,
                     "--error", error,
                     timeout=10)
    for e in events:
        if e.get("type") == "ok":
            return True
    return False


def list_claude_procs():
    events = synapse("chaos", "list_processes", "--pattern", "claude", timeout=10)
    return [e for e in events if e.get("type") == "process"]


def kill_proc(pid):
    events = synapse("chaos", "kill_process", "--pid", str(pid), timeout=10)
    for e in events:
        if e.get("type") == "killed":
            return True
    return False


def graph_snapshot(graph_id):
    return synapse("chaos", "graph_snapshot", "--graph-id", graph_id, timeout=10)


def crash_substrate():
    """Tell the substrate to kill itself. Non-blocking — connection will drop."""
    cmd = [SYNAPSE, "substrate", "chaos", "crash"]
    try:
        subprocess.run(cmd, capture_output=True, text=True, timeout=3, env=LANG_ENV)
    except subprocess.TimeoutExpired:
        pass  # expected — process died


def restart_substrate():
    log("chaos", "Restarting substrate via make...", "yellow")
    result = subprocess.run(
        ["make", "restart"],
        capture_output=True, text=True, timeout=120,
        cwd=SUBSTRATE_DIR, env=LANG_ENV
    )
    if result.returncode == 0:
        log("chaos", "Substrate restarted", "green")
        time.sleep(3)  # give it time to initialize
    else:
        log("chaos", f"make restart failed: {result.stderr[:200]}", "red")


# ─── Main chaos loop ──────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="Orcha Chaos Monkey")
    parser.add_argument("--no-crash", action="store_true",
                        help="Skip the substrate crash test")
    parser.add_argument("--rounds", type=int, default=4,
                        help="Number of chaos rounds before crash (default: 4)")
    parser.add_argument("--graphs", type=int, default=len(GRAPHS),
                        help=f"Number of graphs to launch (default: {len(GRAPHS)})")
    args = parser.parse_args()

    print("\n" + "═" * 60)
    print("  ORCHA CHAOS MONKEY")
    print("═" * 60)

    # ── Phase 1: Launch fleet ──────────────────────────────────────
    print(f"\n[phase 1] Launching {args.graphs} concurrent graphs...\n")
    runs = []
    graphs_to_run = GRAPHS[:args.graphs]

    for g in graphs_to_run:
        run = GraphRun(name=g["name"])
        log(g["name"], g["desc"], "blue")
        launch_graph(run, g["task"])
        runs.append(run)
        time.sleep(1)  # stagger launches slightly

    # ── Phase 2: Chaos rounds ──────────────────────────────────────
    print(f"\n[phase 2] Running {args.rounds} chaos rounds (every 15s)...\n")
    time.sleep(10)  # let nodes reach Running state

    for round_num in range(1, args.rounds + 1):
        print(f"\n{'─' * 40}")
        log("chaos", f"Round {round_num}/{args.rounds}", "yellow")

        running = list_running_nodes()
        log("chaos", f"Found {len(running)} running nodes")

        if running:
            # Inject failure into ~30% of running nodes
            victims = random.sample(running, max(1, len(running) // 3))
            for v in victims:
                gid = v.get("graph_id", "")
                nid = v.get("node_id", "")
                spec = v.get("spec_type", "")
                ok = inject_failure(gid, nid)
                log("chaos", f"inject_failure → {spec} {nid[:8]}... {'✓' if ok else '✗'}", "red")
        else:
            log("chaos", "No running nodes to kill (graphs may be between nodes)")

        # Also occasionally kill a Claude process directly
        if round_num % 2 == 0:
            procs = list_claude_procs()
            if procs:
                victim = random.choice(procs)
                pid = victim.get("pid")
                killed = kill_proc(pid)
                log("chaos", f"kill_process({pid}) → {'✓' if killed else '✗'}", "red")
            else:
                log("chaos", "No Claude processes found")

        # Print fleet status
        print()
        for run in runs:
            symbol = {"complete": "✓", "failed": "✗", "running": "↻",
                      "launching": "…", "timeout": "⏱"}.get(run.status, "?")
            log(run.name, f"{symbol} {run.status}")

        time.sleep(15)

    # ── Phase 3: Substrate crash ───────────────────────────────────
    if not args.no_crash:
        print(f"\n{'═' * 60}")
        log("chaos", "PHASE 3: Hard-crashing the substrate!", "red")

        # Snapshot running graphs before crash
        running_before = list_running_nodes()
        log("chaos", f"{len(running_before)} nodes running at crash time")

        crash_substrate()
        log("chaos", "Substrate killed. Restarting...", "yellow")
        time.sleep(2)

        restart_substrate()

        # Check recovery logs
        log("chaos", "Checking recovery log...", "cyan")
        result = subprocess.run(
            ["grep", "-c", "re-dispatching", "/tmp/substrate.log"],
            capture_output=True, text=True
        )
        count = result.stdout.strip()
        log("chaos", f"Found {count} recovery re-dispatch log entries", "green")

    # ── Phase 4: Wait for completion ──────────────────────────────
    print(f"\n[phase 4] Waiting for graphs to settle (max 5 min)...")
    deadline = time.time() + 300

    while time.time() < deadline:
        still_running = [r for r in runs if r.status == "running"]
        if not still_running:
            break
        time.sleep(10)
        for run in still_running:
            log(run.name, f"still {run.status}...")

    # ── Final report ──────────────────────────────────────────────
    print(f"\n{'═' * 60}")
    print("  RESULTS")
    print("═" * 60)
    complete = [r for r in runs if r.status == "complete"]
    failed   = [r for r in runs if r.status == "failed"]
    timeout  = [r for r in runs if r.status in ("running", "timeout", "launching")]

    for run in runs:
        symbol = {"complete": "✓", "failed": "✗"}.get(run.status, "?")
        color  = {"complete": "green", "failed": "red"}.get(run.status, "yellow")
        log(run.name, f"{symbol} {run.status}", color)
        if run.error:
            log(run.name, f"  error: {run.error[:120]}")

    print()
    print(f"  Complete: {len(complete)}/{len(runs)}")
    print(f"  Failed:   {len(failed)}/{len(runs)}")
    print(f"  Timeout:  {len(timeout)}/{len(runs)}")
    print()

    sys.exit(0 if len(failed) == 0 and len(timeout) == 0 else 1)


if __name__ == "__main__":
    main()
