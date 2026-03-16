use super::types::{OrchaEdgeDef, OrchaNodeDef, OrchaNodeSpec};
use std::collections::HashMap;

// ─── Public API ───────────────────────────────────────────────────────────────

pub struct CompiledGraph {
    pub nodes: Vec<OrchaNodeDef>,
    pub edges: Vec<OrchaEdgeDef>,
}

/// Compile a Markdown plan document into a graph definition.
///
/// # Ticket format
///
/// Each ticket starts at a level-1 heading that includes a `[type]` tag:
///
/// ```markdown
/// # UX-4: Move ir.json Out of Output Directory [agent]
///
/// blocked_by: [UX-2]
/// validate: cargo test -- ir_location_tests
///
/// ## Problem
///
/// The ir.json file is written into the user-facing output directory...
///
/// ## Acceptance Criteria
///
/// - ./generated/ contains only TypeScript files
/// ```
///
/// Everything before the first matching heading is preamble and is ignored.
/// All body content — including `##` subheadings and code blocks — becomes
/// the task prompt (for agent types) or the shell command (for `[prog]`).
///
/// # Supported types
///
/// - `[agent]` — Claude runs the full body as a task prompt
/// - `[agent/synthesize]` — like agent, prepends prior-work context from upstream tokens
/// - `[prog]` — the body (minus metadata lines) is a shell command; exit 0 = pass
///
/// # Body metadata (parsed and stripped from the prompt)
///
/// - `blocked_by: [dep1, dep2]` — dependency list; also accepts `blocked_by: dep1, dep2`
/// - `unlocks: [...]` — informational only, ignored by the compiler
/// - `validate: <shell command>` — auto-generates a sibling `<ID>-validate` node
///
/// # Validate sibling rewriting
///
/// If `UX-4 [agent]` has `validate: cargo test`, the compiler creates:
/// - `UX-4` — Task node
/// - `UX-4-validate` — Validate node running `cargo test`
/// - Any ticket with `blocked_by: [UX-4]` is rewritten to depend on `UX-4-validate`
///   so downstream work only starts after validation passes.
pub fn compile_tickets(input: &str) -> Result<CompiledGraph, String> {
    let sections = parse_sections(input);
    build_graph(sections)
}

// ─── Section parsing ──────────────────────────────────────────────────────────

struct RawSection {
    id: String,
    type_tag: String,
    body_lines: Vec<String>,
}

/// Split the document into per-ticket sections.
///
/// Only `# ID: Title [type]` headings (level 1 with a `[type]` tag and a
/// space-free ID) create new sections.  All other content — including `## `
/// subheadings — is captured as part of the current section's body.
/// Lines before the first ticket heading are preamble and are discarded.
fn parse_sections(input: &str) -> Vec<RawSection> {
    let mut sections: Vec<RawSection> = Vec::new();
    let mut current: Option<(String, String, Vec<String>)> = None;

    for line in input.lines() {
        if let Some((id, type_tag)) = try_parse_ticket_heading(line) {
            if let Some((prev_id, prev_type, lines)) = current.take() {
                sections.push(RawSection { id: prev_id, type_tag: prev_type, body_lines: lines });
            }
            current = Some((id, type_tag, Vec::new()));
        } else if let Some((_, _, ref mut lines)) = current {
            lines.push(line.to_string());
        }
        // else: preamble — skip
    }
    if let Some((id, type_tag, lines)) = current {
        sections.push(RawSection { id, type_tag, body_lines: lines });
    }
    sections
}

/// Try to parse `# ID: Title [type]` → `Some((id, type_tag))`.
///
/// Rules:
/// - Must start with exactly `# ` (not `## `)
/// - Must contain `[type]` somewhere on the line
/// - ID is the first token before `:` or `[`, must be non-empty and contain no spaces
fn try_parse_ticket_heading(line: &str) -> Option<(String, String)> {
    // Exactly "# " — not "## " or deeper
    let rest = line.strip_prefix("# ")?;
    if rest.starts_with('#') {
        return None;
    }
    let rest = rest.trim();

    // Must have [type] bracket
    let bracket_open = rest.find('[')?;
    let after_open = &rest[bracket_open + 1..];
    let bracket_close = after_open.find(']')?;
    let type_tag = after_open[..bracket_close].trim().to_string();
    if type_tag.is_empty() {
        return None;
    }

    // ID: everything before the first '[' or ':', must be a single token (no spaces)
    let before_bracket = rest[..bracket_open].trim();
    let id = before_bracket
        .split(':')
        .next()
        .unwrap_or(before_bracket)
        .trim()
        .to_string();

    if id.is_empty() || id.contains(' ') {
        return None;
    }

    Some((id, type_tag))
}

// ─── Graph building ───────────────────────────────────────────────────────────

struct ParsedTicket {
    id: String,
    type_tag: String,
    deps: Vec<String>,
    /// Task prompt for agent types (body minus metadata lines)
    task: Option<String>,
    /// Shell command for prog types (body minus metadata lines)
    command: Option<String>,
    /// Inline validate command (generates a sibling validate node)
    validate: Option<String>,
}

fn build_graph(sections: Vec<RawSection>) -> Result<CompiledGraph, String> {
    let parsed: Vec<ParsedTicket> = sections
        .into_iter()
        .map(parse_section_body)
        .collect::<Result<_, _>>()?;

    // completion_id maps a ticket id to the id of the last node in its chain.
    // If a ticket has a validate sibling, anything blocked_by that ticket
    // must wait for the validate sibling instead.
    let mut completion_id: HashMap<String, String> = HashMap::new();
    for t in &parsed {
        let effective = if t.validate.is_some() {
            format!("{}-validate", t.id)
        } else {
            t.id.clone()
        };
        completion_id.insert(t.id.clone(), effective);
    }

    let mut nodes: Vec<OrchaNodeDef> = Vec::new();
    let mut edges: Vec<OrchaEdgeDef> = Vec::new();

    for t in &parsed {
        // Primary node
        let spec = match t.type_tag.as_str() {
            "agent" => {
                let task = t.task.clone().ok_or_else(|| {
                    format!("Ticket '{}' [agent] has no body text", t.id)
                })?;
                OrchaNodeSpec::Task { task, max_retries: None }
            }
            "agent/synthesize" => {
                let task = t.task.clone().ok_or_else(|| {
                    format!("Ticket '{}' [agent/synthesize] has no body text", t.id)
                })?;
                OrchaNodeSpec::Synthesize { task, max_retries: None }
            }
            "prog" => {
                let command = t.command.clone().ok_or_else(|| {
                    format!("Ticket '{}' [prog] has no body text", t.id)
                })?;
                OrchaNodeSpec::Validate { command, cwd: None, max_retries: None }
            }
            "review" => {
                let prompt = t.task.clone().ok_or_else(|| {
                    format!("Ticket '{}' [review] has no body text", t.id)
                })?;
                OrchaNodeSpec::Review { prompt }
            }
            "planner" => {
                let task = t.task.clone().ok_or_else(|| {
                    format!("Ticket '{}' [planner] has no body text", t.id)
                })?;
                OrchaNodeSpec::Plan { task }
            }
            other => {
                return Err(format!(
                    "Unknown ticket type [{}] in ticket '{}'",
                    other, t.id
                ))
            }
        };
        nodes.push(OrchaNodeDef { id: t.id.clone(), spec });

        // Validate sibling node
        if let Some(ref cmd) = t.validate {
            nodes.push(OrchaNodeDef {
                id: format!("{}-validate", t.id),
                spec: OrchaNodeSpec::Validate { command: cmd.clone(), cwd: None, max_retries: None },
            });
            // Edge: ticket → validate sibling
            edges.push(OrchaEdgeDef {
                from: t.id.clone(),
                to: format!("{}-validate", t.id),
            });
        }

        // Dependency edges.
        // If the dep is a ticket in this document AND has a validate sibling,
        // rewrite to point at the sibling.  Otherwise pass the id through as-is
        // (allows referencing externally-built lattice nodes).
        for dep in &t.deps {
            let effective_dep = completion_id
                .get(dep)
                .cloned()
                .unwrap_or_else(|| dep.clone());
            edges.push(OrchaEdgeDef { from: effective_dep, to: t.id.clone() });
        }
    }

    Ok(CompiledGraph { nodes, edges })
}

fn parse_section_body(section: RawSection) -> Result<ParsedTicket, String> {
    let RawSection { id, type_tag, body_lines } = section;

    let mut deps: Vec<String> = Vec::new();
    let mut validate: Option<String> = None;
    let mut prose_lines: Vec<String> = Vec::new();

    for line in &body_lines {
        let trimmed = line.trim();

        // Skip comment lines
        if trimmed.starts_with("<!--") || trimmed.starts_with("//") {
            continue;
        }

        // blocked_by: [dep1, dep2]  or  blocked_by: dep1, dep2
        if let Some(rest) = trimmed
            .strip_prefix("blocked_by:")
            .or_else(|| trimmed.strip_prefix("blocked-by:"))
        {
            let list = rest.trim().trim_start_matches('[').trim_end_matches(']');
            deps = list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            continue;
        }

        // validate: <command>  (single line)
        if let Some(cmd) = trimmed.strip_prefix("validate:") {
            let cmd = cmd.trim().to_string();
            if !cmd.is_empty() {
                validate = Some(cmd);
            }
            continue;
        }

        // unlocks: — informational only, discard
        if trimmed.starts_with("unlocks:") {
            continue;
        }

        prose_lines.push(line.to_string());
    }

    // Trim leading/trailing blank lines, preserve internal structure
    let start = prose_lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(prose_lines.len());
    let end = prose_lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    let body = if start < end { prose_lines[start..end].join("\n") } else { String::new() };

    let (task, command) = match type_tag.as_str() {
        "prog" => (None, if body.is_empty() { None } else { Some(body) }),
        _ => (if body.is_empty() { None } else { Some(body) }, None),
    };

    Ok(ParsedTicket { id, type_tag, deps, task, command, validate })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_agent_ticket() {
        let input = "\
# T01: Write the parser [agent]

Implement a JSON webhook parser with typed errors.
";
        let g = compile_tickets(input).unwrap();
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.edges.len(), 0);
        match &g.nodes[0].spec {
            OrchaNodeSpec::Task { task, .. } => assert!(task.contains("JSON webhook parser")),
            _ => panic!("wrong spec"),
        }
    }

    #[test]
    fn test_preamble_is_skipped() {
        let input = "\
# My Epic Plan

This is an overview document with context and background.

## Architecture

Some architecture notes here.

# T01: First ticket [agent]

Do the thing.
";
        let g = compile_tickets(input).unwrap();
        // The first '# My Epic Plan' has no [type], so it's preamble
        assert_eq!(g.nodes.len(), 1);
        assert_eq!(g.nodes[0].id, "T01");
    }

    #[test]
    fn test_validate_sibling() {
        let input = "\
# T01: Write it [agent]

Implement the feature.

blocked_by: []
validate: cargo test -- feature_tests
";
        let g = compile_tickets(input).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert!(g.nodes.iter().any(|n| n.id == "T01"));
        assert!(g.nodes.iter().any(|n| n.id == "T01-validate"));
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0], OrchaEdgeDef { from: "T01".into(), to: "T01-validate".into() });
    }

    #[test]
    fn test_dep_rewriting_through_validate() {
        let input = "\
# T01: First [agent]

Do the first thing.

validate: cargo test -- t01

# T02: Second [agent]

Do the second thing.

blocked_by: [T01]
";
        let g = compile_tickets(input).unwrap();
        // T01, T01-validate, T02
        assert_eq!(g.nodes.len(), 3);

        let edge_pairs: Vec<(&str, &str)> =
            g.edges.iter().map(|e| (e.from.as_str(), e.to.as_str())).collect();

        // T01 → T01-validate (validate sibling edge)
        assert!(edge_pairs.contains(&("T01", "T01-validate")));
        // T02 blocked_by T01 → rewritten to depend on T01-validate
        assert!(edge_pairs.contains(&("T01-validate", "T02")));
        // NOT T01 → T02 directly
        assert!(!edge_pairs.contains(&("T01", "T02")));
    }

    #[test]
    fn test_prog_ticket() {
        let input = "\
# validate-build [prog]

blocked_by: [T01]
cargo build --release 2>&1 | grep -c '^error' | xargs test 0 -eq
";
        let g = compile_tickets(input).unwrap();
        assert_eq!(g.nodes.len(), 1);
        match &g.nodes[0].spec {
            OrchaNodeSpec::Validate { command, .. } => {
                assert!(command.contains("cargo build"));
            }
            _ => panic!("wrong spec"),
        }
    }

    #[test]
    fn test_subsections_become_prose() {
        let input = "\
# UX-4: Move ir.json [agent]

blocked_by: [UX-2]
unlocks: [UX-9]

## Problem

The ir.json file is written into the wrong place.

## Acceptance Criteria

- Output dir contains only TypeScript files
- ir.json lives in cache
";
        let g = compile_tickets(input).unwrap();
        assert_eq!(g.nodes.len(), 1);
        match &g.nodes[0].spec {
            OrchaNodeSpec::Task { task, .. } => {
                assert!(task.contains("## Problem"));
                assert!(task.contains("## Acceptance Criteria"));
                assert!(task.contains("ir.json"));
                // metadata lines are NOT in the prompt
                assert!(!task.contains("blocked_by"));
                assert!(!task.contains("unlocks"));
            }
            _ => panic!("wrong spec"),
        }
        // blocked_by UX-2 parsed correctly
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, "UX-2");
        assert_eq!(g.edges[0].to, "UX-4");
    }

    #[test]
    fn test_synthesize_type() {
        let input = "\
# T03: Synthesize report [agent/synthesize]

Review all prior work and write a final integration report.
";
        let g = compile_tickets(input).unwrap();
        assert!(matches!(&g.nodes[0].spec, OrchaNodeSpec::Synthesize { .. }));
    }

    #[test]
    fn test_multiple_deps() {
        let input = "\
# A [agent]
Task A.

# B [agent]
Task B.

# C [agent]
Task C.

blocked_by: [A, B]
";
        let g = compile_tickets(input).unwrap();
        let edge_pairs: Vec<(&str, &str)> =
            g.edges.iter().map(|e| (e.from.as_str(), e.to.as_str())).collect();
        assert!(edge_pairs.contains(&("A", "C")));
        assert!(edge_pairs.contains(&("B", "C")));
    }
}

impl PartialEq for OrchaEdgeDef {
    fn eq(&self, other: &Self) -> bool {
        self.from == other.from && self.to == other.to
    }
}
