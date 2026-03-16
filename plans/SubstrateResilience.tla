--------------------------- MODULE SubstrateResilience ---------------------------
(*
  Core question:
    For ANY state the graph can be in at hard shutdown, does a valid path
    to completion exist after restart?

  We model three independently-broken recovery mechanisms:

    UseRecovery         - on restart, transition zombie Running→Interrupted.
                          If FALSE: zombie stays Running, no agent, system stalls.

    UseReinit           - when substrate is Up, re-dispatch an Interrupted node
                          (Interrupted→Running with a fresh agent).
                          If FALSE: only AbandonNode is available, which transitions
                          Interrupted→Failed, discarding any partial work.

    UseFailPropagation  - when a dep is Failed, downstream transitions out of
                          Pending (also to Failed, making the graph fully terminal).
                          If FALSE: downstream stays Pending forever with no enabled
                          actions — classic deadlock.

  The Interrupted state is the key distinction from the earlier two-state model:

    Running      - substrate Up, agent alive, work in progress
    Interrupted  - was Running when substrate crashed; agent dead; re-dispatchable
    Failed       - terminal failure; no agent; no recovery at this level

  Recovery matrix (UseRecovery=FALSE → stall regardless of other flags):

    (TRUE,  TRUE,  TRUE)  → optimal: crashed nodes re-run, graph completes
    (TRUE,  FALSE, TRUE)  → degraded: crashed nodes fail via Abandon,
                             propagation clears downstream → graph terminates
    (TRUE,  TRUE,  FALSE) → reinit works for crashes, but natural FailNode
                             leaves downstream Pending deadlocked
    (TRUE,  FALSE, FALSE) → Abandon fails node, propagation absent → deadlock
    (FALSE, *,     *)     → zombie Running blocks all forward progress

  We deliberately do NOT model fine-grained DB write atomicity here.
  Separate concern: we assume advance_graph is transactional (single SQLite txn).
  The crash therefore only produces two ambiguous states:
    Running  - was executing, agent killed, output not persisted
    (all other statuses are post-transaction and self-consistent)
*)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    UseRecovery,
    UseReinit,
    UseFailPropagation

ASSUME UseRecovery        \in BOOLEAN
ASSUME UseReinit          \in BOOLEAN
ASSUME UseFailPropagation \in BOOLEAN

(* ---------------------------------------------------------------------- *)
(* Graph — medium-batch                                                   *)
(* ---------------------------------------------------------------------- *)

Nodes == {"RUNPLAN", "WATCHTREE", "RETRY1", "RETRY2"}

\* <<downstream, upstream>>
Deps == {<<"WATCHTREE", "RUNPLAN">>, <<"RETRY2", "RETRY1">>}

DepsComplete(n, s) ==
    \A pair \in Deps : pair[1] = n => s[pair[2]] = "Complete"

AnyDepFailed(n, s) ==
    \E pair \in Deps : pair[1] = n /\ s[pair[2]] = "Failed"

(* ---------------------------------------------------------------------- *)
(* State                                                                  *)
(* ---------------------------------------------------------------------- *)

VARIABLES
    status,     \* Nodes -> {Pending, Ready, Running, Interrupted, Complete, Failed}
    agent,      \* Nodes -> BOOLEAN  — live agent executing this node
    substrate   \* "Up" | "Down"

vars == <<status, agent, substrate>>

TypeOK ==
    /\ status    \in [Nodes -> {"Pending","Ready","Running","Interrupted","Complete","Failed"}]
    /\ agent     \in [Nodes -> BOOLEAN]
    /\ substrate \in {"Up", "Down"}

Init ==
    /\ status    = [n \in Nodes |-> "Pending"]
    /\ agent     = [n \in Nodes |-> FALSE]
    /\ substrate = "Up"

(* ---------------------------------------------------------------------- *)
(* Normal execution                                                       *)
(* ---------------------------------------------------------------------- *)

BecomeReady(n) ==
    /\ substrate = "Up"
    /\ status[n] = "Pending"
    /\ DepsComplete(n, status)
    /\ status' = [status EXCEPT ![n] = "Ready"]
    /\ UNCHANGED <<agent, substrate>>

StartNode(n) ==
    /\ substrate = "Up"
    /\ status[n] = "Ready"
    /\ status' = [status EXCEPT ![n] = "Running"]
    /\ agent'  = [agent  EXCEPT ![n] = TRUE]
    /\ UNCHANGED substrate

CompleteNode(n) ==
    /\ substrate = "Up"
    /\ status[n] = "Running"
    /\ agent[n]  = TRUE
    /\ status' = [status EXCEPT ![n] = "Complete"]
    /\ agent'  = [agent  EXCEPT ![n] = FALSE]
    /\ UNCHANGED substrate

FailNode(n) ==
    /\ substrate = "Up"
    /\ status[n] = "Running"
    /\ agent[n]  = TRUE
    /\ status' = [status EXCEPT ![n] = "Failed"]
    /\ agent'  = [agent  EXCEPT ![n] = FALSE]
    /\ UNCHANGED substrate

(* ---------------------------------------------------------------------- *)
(* Failure propagation                                                    *)
(*                                                                        *)
(* When a dep has Failed, a Pending/Ready downstream node is also Failed. *)
(* Without this, BecomeReady can never fire (DepsComplete stays FALSE),   *)
(* StartNode has nothing to start, and the graph deadlocks.               *)
(* ---------------------------------------------------------------------- *)

PropagateFail(n) ==
    /\ UseFailPropagation
    /\ substrate = "Up"
    /\ status[n] \in {"Pending", "Ready"}
    /\ AnyDepFailed(n, status)
    /\ status' = [status EXCEPT ![n] = "Failed"]
    /\ UNCHANGED <<agent, substrate>>

(* ---------------------------------------------------------------------- *)
(* Crash / restart                                                        *)
(* ---------------------------------------------------------------------- *)

\* Hard shutdown: kills all agents, leaves DB unchanged.
\* Guard: only crash mid-flight (at least one active node).
Crash ==
    /\ substrate = "Up"
    /\ \E n \in Nodes : status[n] \in {"Ready", "Running"}
    /\ substrate' = "Down"
    /\ agent'     = [n \in Nodes |-> FALSE]
    /\ UNCHANGED status

\* Restart — two variants controlled by UseRecovery:
\*
\*   UseRecovery = TRUE:
\*     Every zombie Running node (status=Running, agent=FALSE because agents died)
\*     is transitioned to Interrupted.  An Interrupted node has no agent but is
\*     not yet Failed — it can be re-dispatched (ReinitNode) or abandoned (AbandonNode).
\*
\*   UseRecovery = FALSE (bug — plexus_substrate filter doesn't match crate name):
\*     DB state is unchanged.  Zombie Running nodes persist.
\*     No forward action is enabled for them: CompleteNode/FailNode require
\*     agent=TRUE, but agents are dead.  Graph stalls indefinitely.
Restart ==
    /\ substrate = "Down"
    /\ substrate' = "Up"
    /\ IF UseRecovery
       THEN \* Correct recovery: zombie Running → Interrupted (re-dispatchable)
            status' = [n \in Nodes |->
                          IF status[n] = "Running" THEN "Interrupted"
                          ELSE status[n]]
       ELSE \* Bug: recovery silently skipped
            UNCHANGED status
    /\ UNCHANGED agent

(* ---------------------------------------------------------------------- *)
(* Interrupted node resolution                                            *)
(*                                                                        *)
(* An Interrupted node (Running in DB, agent dead after crash) has two    *)
(* possible forward transitions once the substrate is back Up:            *)
(*                                                                        *)
(*   ReinitNode  - re-dispatch: spawn a fresh agent, resume execution.    *)
(*                 Requires UseReinit=TRUE.                                *)
(*                                                                        *)
(*   AbandonNode - give up: mark Failed, which unblocks PropagateFail     *)
(*                 downstream.  Always available (UseReinit is irrelevant).*)
(*                 In practice this is the safe fallback when reinit       *)
(*                 cannot be attempted (e.g., agent spawn itself fails).   *)
(* ---------------------------------------------------------------------- *)

ReinitNode(n) ==
    /\ UseReinit
    /\ substrate = "Up"
    /\ status[n] = "Interrupted"
    /\ status' = [status EXCEPT ![n] = "Running"]
    /\ agent'  = [agent  EXCEPT ![n] = TRUE]
    /\ UNCHANGED substrate

AbandonNode(n) ==
    /\ substrate = "Up"
    /\ status[n] = "Interrupted"
    /\ status' = [status EXCEPT ![n] = "Failed"]
    /\ UNCHANGED <<agent, substrate>>

(* ---------------------------------------------------------------------- *)
(* Terminal                                                               *)
(* ---------------------------------------------------------------------- *)

AllDone == \A n \in Nodes : status[n] \in {"Complete", "Failed"}

Terminating ==
    /\ AllDone /\ substrate = "Up"
    /\ UNCHANGED vars

(* ---------------------------------------------------------------------- *)
(* Spec                                                                   *)
(* ---------------------------------------------------------------------- *)

Next ==
    \/ \E n \in Nodes : BecomeReady(n)
    \/ \E n \in Nodes : StartNode(n)
    \/ \E n \in Nodes : CompleteNode(n)
    \/ \E n \in Nodes : FailNode(n)
    \/ \E n \in Nodes : PropagateFail(n)
    \/ \E n \in Nodes : ReinitNode(n)
    \/ \E n \in Nodes : AbandonNode(n)
    \/ Crash
    \/ Restart
    \/ Terminating

\* WF ensures the scheduler doesn't permanently ignore enabled actions.
\* SF on progress actions ensures nodes that *can* complete eventually *do* —
\* i.e., the substrate doesn't crash infinitely often before any node finishes.
\* (A substrate that crashes on every single step is not a useful system.)
\*
\* SF on ReinitNode: if UseReinit=TRUE, an Interrupted node is eventually
\* re-dispatched.  If UseReinit=FALSE, ReinitNode is never enabled, so this
\* clause is vacuously satisfied.
\*
\* WF on AbandonNode: if an Interrupted node is never reinit'd (UseReinit=FALSE,
\* or reinit is blocked), it is eventually abandoned → Failed → propagation.
Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Next)
    /\ \A n \in Nodes : SF_vars(StartNode(n))    \* ready nodes eventually start
    /\ \A n \in Nodes : SF_vars(CompleteNode(n)) \* started nodes eventually finish
    /\ \A n \in Nodes : SF_vars(FailNode(n))     \* or fail
    /\ SF_vars(Restart)                          \* substrate eventually comes back
    /\ \A n \in Nodes : SF_vars(ReinitNode(n))   \* interrupted nodes eventually reinit (if enabled)
    /\ \A n \in Nodes : WF_vars(AbandonNode(n))  \* or eventually abandoned

(* ---------------------------------------------------------------------- *)
(* Safety                                                                 *)
(* ---------------------------------------------------------------------- *)

\* Every Running node must have a live agent behind it.
\* Crash leaves zombie Running nodes — violated when UseRecovery=FALSE.
\* Interrupted nodes legitimately have no agent; that is the point of the state.
AgentConsistency ==
    \A n \in Nodes :
        (substrate = "Up" /\ status[n] = "Running") => agent[n] = TRUE

\* Interrupted nodes must not have a live agent (Crash clears them; Reinit
\* transitions immediately to Running with a fresh one).
InterruptedHasNoAgent ==
    \A n \in Nodes : status[n] = "Interrupted" => ~agent[n]

DepsRespected ==
    \A pair \in Deps :
        status[pair[1]] = "Running" => status[pair[2]] = "Complete"

NoAgentsWhileDown ==
    substrate = "Down" => \A n \in Nodes : ~agent[n]

(* ---------------------------------------------------------------------- *)
(* Liveness                                                               *)
(* ---------------------------------------------------------------------- *)

\* The graph eventually finishes (all nodes terminal).
\* Fails unless UseRecovery=TRUE AND UseFailPropagation=TRUE AND
\* (UseReinit=TRUE OR AbandonNode fairness covers the Interrupted→Failed path).
EventualCompletion == <>(AllDone)

EachNodeTerminates ==
    /\ <>(status["RUNPLAN"]   \in {"Complete","Failed"})
    /\ <>(status["WATCHTREE"] \in {"Complete","Failed"})
    /\ <>(status["RETRY1"]    \in {"Complete","Failed"})
    /\ <>(status["RETRY2"]    \in {"Complete","Failed"})

=============================================================================
