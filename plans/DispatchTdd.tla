---------------------------- MODULE DispatchTdd ----------------------------
(*
  Formal specification of the dispatch_tdd orchestration loop.

  Models the control flow of a single TDD node: spec phase, parallel
  impl+test branches, validation, and the repair/escalation loop.

  This spec deliberately abstracts over agent *content* — whether Claude
  produced correct code — and focuses on the *orchestration invariants*:
    - Branches never run before a validated contract exists
    - Impossible specs never silently retry — they escalate to humans
    - Cycle counts are strictly bounded
    - The node always terminates (Complete or Failed)

  Run with TLC:
    java -jar tla2tools.jar -workers 4 -config DispatchTdd.cfg DispatchTdd.tla

  TLC model values:
    MaxRepairCycles <- 2
    MaxSpecCycles   <- 2
*)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
    MaxRepairCycles,   \* Upper bound on repair loop iterations
    MaxSpecCycles      \* Upper bound on contract refinement iterations

ASSUME MaxRepairCycles \in Nat /\ MaxRepairCycles >= 1
ASSUME MaxSpecCycles   \in Nat /\ MaxSpecCycles   >= 1

(* ---------------------------------------------------------------------- *)
(* State space                                                             *)
(* ---------------------------------------------------------------------- *)

Phases == {
    "Idle",
    "ContractPhase",        \* Spec agent running
    "ContractValidating",   \* TLC running against the spec
    "Branching",            \* Impl + test agents running in parallel
    "Validating",           \* cargo test (or project validate_command) running
    "Repairing",            \* Repair agent classifying the failure
    "EscalatingToHuman",    \* Waiting for human approval via loopback
    "Complete",
    "Failed"
}

\* Diagnoses the repair agent can produce
Diagnoses == {
    "ImplBug",           \* Impl is wrong; contract and tests agree
    "TestBug",           \* Test is wrong; impl is correct per contract
    "ImplTestMismatch",  \* Both diverged from contract incompatibly
    "ContractAmbiguity", \* Spec didn't pin down the behavior; must refine
    "Impossible",        \* Spec cannot be satisfied in this codebase
    "Environmental"      \* Missing dep, wrong path — not a logic failure
}

Null == "Null"

VARIABLES
    phase,          \* Current phase \in Phases
    repair_cycle,   \* Number of repair iterations so far
    spec_cycle,     \* Number of contract spec/refinement iterations so far
    contract,       \* Null | "present"  (artifact in pm storage)
    spec_valid,     \* Null | "pass" | "fail"  (TLC result for current spec)
    branches_done,  \* Subset of {"impl","test"} — which branches completed
    validate_result,\* Null | "pass" | "fail"
    diagnosis,      \* Null | element of Diagnoses
    human_response  \* Null | "approved" | "denied"

vars == <<phase, repair_cycle, spec_cycle, contract, spec_valid,
          branches_done, validate_result, diagnosis, human_response>>

(* ---------------------------------------------------------------------- *)
(* Type invariant                                                          *)
(* ---------------------------------------------------------------------- *)

TypeOK ==
    /\ phase           \in Phases
    /\ repair_cycle    \in 0..MaxRepairCycles
    /\ spec_cycle      \in 0..MaxSpecCycles
    /\ contract        \in {Null, "present"}
    /\ spec_valid      \in {Null, "pass", "fail"}
    /\ branches_done   \subseteq {"impl", "test"}
    /\ validate_result \in {Null, "pass", "fail"}
    /\ diagnosis       \in ({Null} \union Diagnoses)
    /\ human_response  \in {Null, "approved", "denied"}

(* ---------------------------------------------------------------------- *)
(* Initial state                                                           *)
(* ---------------------------------------------------------------------- *)

Init ==
    /\ phase           = "Idle"
    /\ repair_cycle    = 0
    /\ spec_cycle      = 0
    /\ contract        = Null
    /\ spec_valid      = Null
    /\ branches_done   = {}
    /\ validate_result = Null
    /\ diagnosis       = Null
    /\ human_response  = Null

(* ---------------------------------------------------------------------- *)
(* Actions — Phase 1: Contract                                             *)
(* ---------------------------------------------------------------------- *)

\* Kick off the TDD node
Start ==
    /\ phase = "Idle"
    /\ phase' = "ContractPhase"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis, human_response>>

\* Spec agent completes and produces a contract artifact
ContractComplete ==
    /\ phase    = "ContractPhase"
    /\ phase'   = "ContractValidating"
    /\ contract' = "present"
    /\ spec_valid' = Null   \* Reset: about to run TLC on this new spec
    /\ UNCHANGED <<repair_cycle, spec_cycle, branches_done,
                   validate_result, diagnosis, human_response>>

\* TLC passes — spec is internally consistent
SpecPass ==
    /\ phase      = "ContractValidating"
    /\ contract   = "present"
    /\ phase'     = "Branching"
    /\ spec_valid' = "pass"
    /\ branches_done' = {}  \* Fresh start for this branch cycle
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract,
                   validate_result, diagnosis, human_response>>

\* TLC finds a counter-example — spec is self-contradictory; refine it
SpecFail ==
    /\ phase      = "ContractValidating"
    /\ contract   = "present"
    /\ spec_cycle < MaxSpecCycles
    /\ phase'      = "ContractPhase"
    /\ spec_valid' = "fail"
    /\ spec_cycle' = spec_cycle + 1
    /\ contract'   = Null   \* Must re-derive the contract
    /\ UNCHANGED <<repair_cycle, branches_done, validate_result,
                   diagnosis, human_response>>

\* Spec has been refined too many times — cannot produce a valid spec
SpecExhausted ==
    /\ phase      = "ContractValidating"
    /\ contract   = "present"
    /\ spec_cycle >= MaxSpecCycles
    /\ phase'      = "Failed"
    /\ spec_valid' = "fail"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, branches_done,
                   validate_result, diagnosis, human_response>>

(* ---------------------------------------------------------------------- *)
(* Actions — Phase 2: Parallel branches                                   *)
(* ---------------------------------------------------------------------- *)

\* Impl agent finishes (order-independent with TestComplete)
ImplComplete ==
    /\ phase = "Branching"
    /\ "impl" \notin branches_done
    /\ branches_done' = branches_done \union {"impl"}
    /\ UNCHANGED <<phase, repair_cycle, spec_cycle, contract, spec_valid,
                   validate_result, diagnosis, human_response>>

\* Test agent finishes
TestComplete ==
    /\ phase = "Branching"
    /\ "test" \notin branches_done
    /\ branches_done' = branches_done \union {"test"}
    /\ UNCHANGED <<phase, repair_cycle, spec_cycle, contract, spec_valid,
                   validate_result, diagnosis, human_response>>

\* Gather: both branches done — advance to validation
BranchesGather ==
    /\ phase         = "Branching"
    /\ branches_done = {"impl", "test"}
    /\ phase' = "Validating"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis, human_response>>

(* ---------------------------------------------------------------------- *)
(* Actions — Phase 3: Validation                                          *)
(* ---------------------------------------------------------------------- *)

\* validate_command exits 0
ValidatePass ==
    /\ phase = "Validating"
    /\ phase'          = "Complete"
    /\ validate_result' = "pass"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, diagnosis, human_response>>

\* validate_command exits non-zero and repair cycles remain
ValidateFail ==
    /\ phase        = "Validating"
    /\ repair_cycle < MaxRepairCycles
    /\ phase'          = "Repairing"
    /\ validate_result' = "fail"
    /\ diagnosis'       = Null   \* Repair agent hasn't classified yet
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, human_response>>

\* Out of repair cycles
ValidateExhausted ==
    /\ phase        = "Validating"
    /\ repair_cycle >= MaxRepairCycles
    /\ phase'          = "Failed"
    /\ validate_result' = "fail"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, diagnosis, human_response>>

(* ---------------------------------------------------------------------- *)
(* Actions — Phase 4: Repair                                              *)
(* ---------------------------------------------------------------------- *)

\* Repair agent produces a diagnosis (nondeterministic — TLC explores all)
RepairDiagnose ==
    /\ phase     = "Repairing"
    /\ diagnosis = Null
    /\ \E d \in Diagnoses : diagnosis' = d
    /\ UNCHANGED <<phase, repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, human_response>>

\* Fixable failures: re-run branches with error context
RepairRebranch ==
    /\ phase      = "Repairing"
    /\ diagnosis \in {"ImplBug", "TestBug", "ImplTestMismatch", "Environmental"}
    /\ phase'         = "Branching"
    /\ repair_cycle'  = repair_cycle + 1
    /\ branches_done' = {}
    /\ validate_result' = Null
    /\ diagnosis'     = Null
    /\ UNCHANGED <<spec_cycle, contract, spec_valid, human_response>>

\* Contract ambiguity: refine the spec (if spec cycles remain)
RepairRefineContract ==
    /\ phase      = "Repairing"
    /\ diagnosis  = "ContractAmbiguity"
    /\ spec_cycle < MaxSpecCycles
    /\ phase'          = "ContractPhase"
    /\ repair_cycle'   = repair_cycle + 1
    /\ spec_cycle'     = spec_cycle + 1
    /\ contract'       = Null
    /\ spec_valid'     = Null
    /\ branches_done'  = {}
    /\ validate_result' = Null
    /\ diagnosis'      = Null
    /\ UNCHANGED <<human_response>>

\* Contract ambiguity but spec cycles exhausted — must escalate
RepairAmbiguityExhausted ==
    /\ phase      = "Repairing"
    /\ diagnosis  = "ContractAmbiguity"
    /\ spec_cycle >= MaxSpecCycles
    /\ phase' = "EscalatingToHuman"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis, human_response>>

\* Impossible spec or unresolvable failure — escalate to human
RepairEscalate ==
    /\ phase     = "Repairing"
    /\ diagnosis = "Impossible"
    /\ phase' = "EscalatingToHuman"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis, human_response>>

(* ---------------------------------------------------------------------- *)
(* Actions — Human gate                                                   *)
(* ---------------------------------------------------------------------- *)

\* Human reviews the failure and approves another attempt
HumanApprove ==
    /\ phase = "EscalatingToHuman"
    /\ repair_cycle < MaxRepairCycles
    /\ human_response' = "approved"
    /\ phase'          = "Branching"
    /\ repair_cycle'   = repair_cycle + 1
    /\ branches_done'  = {}
    /\ validate_result' = Null
    /\ diagnosis'      = Null
    /\ UNCHANGED <<spec_cycle, contract, spec_valid>>

\* Human denies — hard stop
HumanDeny ==
    /\ phase = "EscalatingToHuman"
    /\ human_response' = "denied"
    /\ phase' = "Failed"
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis>>

\* Terminal states are absorbing — explicit stuttering prevents TLC deadlock reports
Terminating ==
    /\ phase \in {"Complete", "Failed"}
    /\ UNCHANGED vars

\* Human approves but repair cycles exhausted — can't continue
HumanApproveExhausted ==
    /\ phase = "EscalatingToHuman"
    /\ repair_cycle >= MaxRepairCycles
    /\ phase' = "Failed"
    /\ human_response' = "approved"   \* Recorded but cannot act
    /\ UNCHANGED <<repair_cycle, spec_cycle, contract, spec_valid,
                   branches_done, validate_result, diagnosis>>

(* ---------------------------------------------------------------------- *)
(* Specification                                                           *)
(* ---------------------------------------------------------------------- *)

Next ==
    \/ Start
    \/ ContractComplete
    \/ SpecPass
    \/ SpecFail
    \/ SpecExhausted
    \/ ImplComplete
    \/ TestComplete
    \/ BranchesGather
    \/ ValidatePass
    \/ ValidateFail
    \/ ValidateExhausted
    \/ RepairDiagnose
    \/ RepairRebranch
    \/ RepairRefineContract
    \/ RepairAmbiguityExhausted
    \/ RepairEscalate
    \/ HumanApprove
    \/ HumanDeny
    \/ HumanApproveExhausted
    \/ Terminating

\* Weak fairness: if an action is continuously enabled it eventually fires.
\* This rules out infinite stuttering (e.g. stuck in Branching forever).
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

(* ---------------------------------------------------------------------- *)
(* Safety properties                                                       *)
(*                                                                         *)
(* Each property comes in two forms:                                       *)
(*   Foo    — state predicate, checked by TLC as an INVARIANT             *)
(*   FooAlways — temporal wrapper []Foo, listed in PROPERTIES             *)
(* ---------------------------------------------------------------------- *)

\* Branches never start without a validated contract
BranchingRequiresValidContract ==
    phase = "Branching" => contract = "present" /\ spec_valid = "pass"

NoBranchingBeforeValidContract ==
    [](BranchingRequiresValidContract)

\* Validation never starts until both branches have completed
ValidationRequiresBothBranches ==
    phase = "Validating" => branches_done = {"impl", "test"}

NoValidationBeforeBothBranches ==
    [](ValidationRequiresBothBranches)

\* An "Impossible" diagnosis always routes to human — never silently retries
ImpossibleRoutesToHuman ==
    diagnosis = "Impossible" =>
        phase \in {"Repairing", "EscalatingToHuman", "Failed"}

ImpossibleAlwaysEscalates ==
    [](ImpossibleRoutesToHuman)

\* Repair cycles only go up, never down (action property — uses primes)
RepairCycleMonotone ==
    [][repair_cycle' >= repair_cycle]_vars

\* Spec cycles only go up
SpecCycleMonotone ==
    [][spec_cycle' >= spec_cycle]_vars

\* Bounds are always respected
RepairCycleInBounds == repair_cycle <= MaxRepairCycles
SpecCycleInBounds   == spec_cycle   <= MaxSpecCycles

CyclesBounded ==
    /\ [](RepairCycleInBounds)
    /\ [](SpecCycleInBounds)

\* The contract must be present whenever agents are doing real work
ContractPresentDuringWork ==
    phase \in {"Branching", "Validating", "Repairing", "EscalatingToHuman"}
        => contract = "present"

ContractAlwaysPresentDuringWork ==
    [](ContractPresentDuringWork)

\* Terminal states are absorbing (action properties — use primes)
CompleteIsTerminal ==
    [][phase = "Complete" => phase' = "Complete"]_vars

FailedIsTerminal ==
    [][phase = "Failed" => phase' = "Failed"]_vars

(* ---------------------------------------------------------------------- *)
(* Liveness properties — good things eventually happen                    *)
(* ---------------------------------------------------------------------- *)

\* The node always eventually terminates
EventualTermination ==
    <>(phase \in {"Complete", "Failed"})

\* Human engagement leads to resolution
HumanGateResolves ==
    (phase = "EscalatingToHuman") ~> (phase \in {"Branching", "Failed"})

\* A passing validation always leads to Complete
ValidPassLeadsToComplete ==
    (validate_result = "pass") ~> (phase = "Complete")

\* The node can in principle succeed (reachability — checked via TLC)
\* Verified by TLC finding at least one path to Complete.

(* ---------------------------------------------------------------------- *)
(* Theorems — checked exhaustively by TLC                                 *)
(* ---------------------------------------------------------------------- *)

THEOREM Spec => TypeOK
THEOREM Spec => NoBranchingBeforeValidContract
THEOREM Spec => NoValidationBeforeBothBranches
THEOREM Spec => ImpossibleAlwaysEscalates
THEOREM Spec => CyclesBounded
THEOREM Spec => ContractAlwaysPresentDuringWork
THEOREM Spec => CompleteIsTerminal
THEOREM Spec => FailedIsTerminal
THEOREM Spec => EventualTermination
THEOREM Spec => HumanGateResolves
THEOREM Spec => ValidPassLeadsToComplete

=============================================================================
