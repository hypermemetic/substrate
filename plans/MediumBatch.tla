---------------------------- MODULE MediumBatch ----------------------------
(*
  Validates the medium-batch ticket graph before submission to orcha.

  Models two orthogonal concerns:
    1. Logical dependencies (blocked_by) — a ticket can't run until deps complete
    2. File resource conflicts — two tickets modifying the same file can't run
       simultaneously or the second write overwrites the first

  We run this in two configurations via MediumBatch.cfg:
    UseFileLocks = FALSE  →  exposes the RUNPLAN/WATCHTREE conflict
    UseFileLocks = TRUE   →  verifies the fix (file-aware StartRunning)
*)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    UseFileLocks    \* TRUE = enforce file exclusion in StartRunning

ASSUME UseFileLocks \in BOOLEAN

(* ---------------------------------------------------------------------- *)
(* Ticket graph — matches medium-batch.tickets.md                         *)
(* ---------------------------------------------------------------------- *)

Tickets == {"RUNPLAN", "WATCHTREE", "RETRY1", "RETRY2"}

\* Logical dependencies: <<dependent, dependency>>
\* RETRY2 blocked_by RETRY1; all others independent
LogicalDeps == {<<"RETRY2", "RETRY1">>}

\* Files each ticket modifies
FileOf(t) ==
    IF      t = "RUNPLAN"   THEN {"activation.rs"}
    ELSE IF t = "WATCHTREE" THEN {"activation.rs"}
    ELSE IF t = "RETRY1"    THEN {"types.rs", "graph_runtime.rs"}
    ELSE IF t = "RETRY2"    THEN {"graph_runner.rs"}
    ELSE    {}

AllFiles == UNION {FileOf(t) : t \in Tickets}

(* ---------------------------------------------------------------------- *)
(* Helpers                                                                *)
(* ---------------------------------------------------------------------- *)

\* All logical dependencies of ticket t have completed
DepsComplete(t, s) ==
    \A pair \in LogicalDeps : pair[1] = t => s[pair[2]] = "Complete"

\* Set of files currently held by Running tickets
LockedFiles(s) ==
    UNION {FileOf(t) : t \in {x \in Tickets : s[x] = "Running"}}

\* Files needed by t are not currently locked
FilesAvailable(t, s) ==
    FileOf(t) \cap LockedFiles(s) = {}

(* ---------------------------------------------------------------------- *)
(* State                                                                   *)
(* ---------------------------------------------------------------------- *)

VARIABLES status   \* status : Tickets -> {Pending, Ready, Running, Complete}

vars == <<status>>

Statuses == {"Pending", "Ready", "Running", "Complete"}

TypeOK == status \in [Tickets -> Statuses]

(* ---------------------------------------------------------------------- *)
(* Initial state — all tickets pending                                     *)
(* ---------------------------------------------------------------------- *)

Init == status = [t \in Tickets |-> "Pending"]

(* ---------------------------------------------------------------------- *)
(* Actions                                                                 *)
(* ---------------------------------------------------------------------- *)

\* A pending ticket becomes ready once all its logical deps are complete
BecomeReady(t) ==
    /\ status[t] = "Pending"
    /\ DepsComplete(t, status)
    /\ status' = [status EXCEPT ![t] = "Ready"]

\* A ready ticket starts running.
\* When UseFileLocks=TRUE, it also requires no file conflicts.
\* When UseFileLocks=FALSE, file conflicts are silently allowed — exposes the bug.
StartRunning(t) ==
    /\ status[t] = "Ready"
    /\ (UseFileLocks => FilesAvailable(t, status))
    /\ status' = [status EXCEPT ![t] = "Running"]

\* A running ticket completes (releases its implicit file locks)
Complete(t) ==
    /\ status[t] = "Running"
    /\ status' = [status EXCEPT ![t] = "Complete"]

\* Stuttering in terminal state
Terminating ==
    /\ \A t \in Tickets : status[t] = "Complete"
    /\ UNCHANGED vars

Next ==
    \/ \E t \in Tickets : BecomeReady(t)
    \/ \E t \in Tickets : StartRunning(t)
    \/ \E t \in Tickets : Complete(t)
    \/ Terminating

\* Weak fairness: every continuously-enabled action eventually fires
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(* ---------------------------------------------------------------------- *)
(* Safety properties                                                       *)
(* ---------------------------------------------------------------------- *)

\* No two running tickets share a file (the core conflict check)
NoFileConflict ==
    \A f \in AllFiles :
        Cardinality({t \in Tickets : status[t] = "Running" /\ f \in FileOf(t)}) <= 1

\* A ticket only runs after all its logical deps complete
DepsRespected ==
    \A pair \in LogicalDeps :
        status[pair[1]] = "Running" => status[pair[2]] = "Complete"

\* RETRY2 never starts before RETRY1 is done
RETRY2AfterRETRY1 ==
    status["RETRY2"] \in {"Running", "Complete"} =>
        status["RETRY1"] = "Complete"

(* ---------------------------------------------------------------------- *)
(* Liveness                                                                *)
(* ---------------------------------------------------------------------- *)

AllComplete == \A t \in Tickets : status[t] = "Complete"

\* The whole batch eventually finishes
EventualCompletion == <>AllComplete

\* Each individual ticket eventually completes
EachTicketCompletes ==
    /\ <>(status["RUNPLAN"]   = "Complete")
    /\ <>(status["WATCHTREE"] = "Complete")
    /\ <>(status["RETRY1"]    = "Complete")
    /\ <>(status["RETRY2"]    = "Complete")

=============================================================================
