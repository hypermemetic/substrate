# Chaos

Fault injection and observability for anti-fragility testing. Lets you force-fail
or force-complete running Lattice nodes, inspect process lists, kill processes,
and crash substrate itself.

Used for testing graph recovery, retry loops, and failure propagation.

---

## Hub methods

**Namespace:** `chaos`

| Method | Params | Returns |
|--------|--------|---------|
| `list_running_nodes` | — | `Stream<ListRunningResult>` — all Running nodes across all graphs |
| `inject_failure` | `graph_id, node_id, error?` | `Stream<InjectResult>` — force-fail a Running node |
| `inject_success` | `graph_id, node_id, value?` | `Stream<InjectResult>` — force-complete with ok token |
| `list_processes` | `pattern` | `Stream<ListProcessesResult>` — PIDs matching cmdline substring |
| `kill_process` | `pid: u32` | `Stream<KillProcessResult>` — SIGKILL |
| `graph_snapshot` | `graph_id` | `Stream<GraphSnapshotResult>` — all nodes + aggregate counts |
| `crash` | — | Logs warning, flushes response, then kills substrate (SIGKILL self) |

---

## Notes

- `inject_failure` and `inject_success` call `lattice.advance_graph` with the
  appropriate token — the same path as normal node completion.
- Skips nodes not in `Running` state.
- `list_processes` scans `/proc/*/cmdline` for substring matches — useful for
  finding Claude Code subprocesses.
- `crash` is for recovery testing: verifies substrate restarts cleanly and
  reconnects to running graphs.
