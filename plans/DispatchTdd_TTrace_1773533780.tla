---- MODULE DispatchTdd_TTrace_1773533780 ----
EXTENDS Sequences, TLCExt, Toolbox, DispatchTdd, Naturals, TLC

_expression ==
    LET DispatchTdd_TEExpression == INSTANCE DispatchTdd_TEExpression
    IN DispatchTdd_TEExpression!expression
----

_trace ==
    LET DispatchTdd_TETrace == INSTANCE DispatchTdd_TETrace
    IN DispatchTdd_TETrace!trace
----

_inv ==
    ~(
        TLCGet("level") = Len(_TETrace)
        /\
        phase = ("Complete")
        /\
        human_response = ("Null")
        /\
        validate_result = ("pass")
        /\
        contract = ("present")
        /\
        spec_cycle = (0)
        /\
        branches_done = ({"impl", "test"})
        /\
        diagnosis = ("Null")
        /\
        repair_cycle = (0)
        /\
        spec_valid = ("pass")
    )
----

_init ==
    /\ phase = _TETrace[1].phase
    /\ contract = _TETrace[1].contract
    /\ branches_done = _TETrace[1].branches_done
    /\ spec_valid = _TETrace[1].spec_valid
    /\ human_response = _TETrace[1].human_response
    /\ validate_result = _TETrace[1].validate_result
    /\ spec_cycle = _TETrace[1].spec_cycle
    /\ diagnosis = _TETrace[1].diagnosis
    /\ repair_cycle = _TETrace[1].repair_cycle
----

_next ==
    /\ \E i,j \in DOMAIN _TETrace:
        /\ \/ /\ j = i + 1
              /\ i = TLCGet("level")
        /\ phase  = _TETrace[i].phase
        /\ phase' = _TETrace[j].phase
        /\ contract  = _TETrace[i].contract
        /\ contract' = _TETrace[j].contract
        /\ branches_done  = _TETrace[i].branches_done
        /\ branches_done' = _TETrace[j].branches_done
        /\ spec_valid  = _TETrace[i].spec_valid
        /\ spec_valid' = _TETrace[j].spec_valid
        /\ human_response  = _TETrace[i].human_response
        /\ human_response' = _TETrace[j].human_response
        /\ validate_result  = _TETrace[i].validate_result
        /\ validate_result' = _TETrace[j].validate_result
        /\ spec_cycle  = _TETrace[i].spec_cycle
        /\ spec_cycle' = _TETrace[j].spec_cycle
        /\ diagnosis  = _TETrace[i].diagnosis
        /\ diagnosis' = _TETrace[j].diagnosis
        /\ repair_cycle  = _TETrace[i].repair_cycle
        /\ repair_cycle' = _TETrace[j].repair_cycle

\* Uncomment the ASSUME below to write the states of the error trace
\* to the given file in Json format. Note that you can pass any tuple
\* to `JsonSerialize`. For example, a sub-sequence of _TETrace.
    \* ASSUME
    \*     LET J == INSTANCE Json
    \*         IN J!JsonSerialize("DispatchTdd_TTrace_1773533780.json", _TETrace)

=============================================================================

 Note that you can extract this module `DispatchTdd_TEExpression`
  to a dedicated file to reuse `expression` (the module in the 
  dedicated `DispatchTdd_TEExpression.tla` file takes precedence 
  over the module `DispatchTdd_TEExpression` below).

---- MODULE DispatchTdd_TEExpression ----
EXTENDS Sequences, TLCExt, Toolbox, DispatchTdd, Naturals, TLC

expression == 
    [
        \* To hide variables of the `DispatchTdd` spec from the error trace,
        \* remove the variables below.  The trace will be written in the order
        \* of the fields of this record.
        phase |-> phase
        ,contract |-> contract
        ,branches_done |-> branches_done
        ,spec_valid |-> spec_valid
        ,human_response |-> human_response
        ,validate_result |-> validate_result
        ,spec_cycle |-> spec_cycle
        ,diagnosis |-> diagnosis
        ,repair_cycle |-> repair_cycle
        
        \* Put additional constant-, state-, and action-level expressions here:
        \* ,_stateNumber |-> _TEPosition
        \* ,_phaseUnchanged |-> phase = phase'
        
        \* Format the `phase` variable as Json value.
        \* ,_phaseJson |->
        \*     LET J == INSTANCE Json
        \*     IN J!ToJson(phase)
        
        \* Lastly, you may build expressions over arbitrary sets of states by
        \* leveraging the _TETrace operator.  For example, this is how to
        \* count the number of times a spec variable changed up to the current
        \* state in the trace.
        \* ,_phaseModCount |->
        \*     LET F[s \in DOMAIN _TETrace] ==
        \*         IF s = 1 THEN 0
        \*         ELSE IF _TETrace[s].phase # _TETrace[s-1].phase
        \*             THEN 1 + F[s-1] ELSE F[s-1]
        \*     IN F[_TEPosition - 1]
    ]

=============================================================================



Parsing and semantic processing can take forever if the trace below is long.
 In this case, it is advised to uncomment the module below to deserialize the
 trace from a generated binary file.

\*
\*---- MODULE DispatchTdd_TETrace ----
\*EXTENDS IOUtils, DispatchTdd, TLC
\*
\*trace == IODeserialize("DispatchTdd_TTrace_1773533780.bin", TRUE)
\*
\*=============================================================================
\*

---- MODULE DispatchTdd_TETrace ----
EXTENDS DispatchTdd, TLC

trace == 
    <<
    ([phase |-> "Idle",human_response |-> "Null",validate_result |-> "Null",contract |-> "Null",spec_cycle |-> 0,branches_done |-> {},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "Null"]),
    ([phase |-> "ContractPhase",human_response |-> "Null",validate_result |-> "Null",contract |-> "Null",spec_cycle |-> 0,branches_done |-> {},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "Null"]),
    ([phase |-> "ContractValidating",human_response |-> "Null",validate_result |-> "Null",contract |-> "present",spec_cycle |-> 0,branches_done |-> {},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "Null"]),
    ([phase |-> "Branching",human_response |-> "Null",validate_result |-> "Null",contract |-> "present",spec_cycle |-> 0,branches_done |-> {},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "pass"]),
    ([phase |-> "Branching",human_response |-> "Null",validate_result |-> "Null",contract |-> "present",spec_cycle |-> 0,branches_done |-> {"impl"},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "pass"]),
    ([phase |-> "Branching",human_response |-> "Null",validate_result |-> "Null",contract |-> "present",spec_cycle |-> 0,branches_done |-> {"impl", "test"},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "pass"]),
    ([phase |-> "Validating",human_response |-> "Null",validate_result |-> "Null",contract |-> "present",spec_cycle |-> 0,branches_done |-> {"impl", "test"},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "pass"]),
    ([phase |-> "Complete",human_response |-> "Null",validate_result |-> "pass",contract |-> "present",spec_cycle |-> 0,branches_done |-> {"impl", "test"},diagnosis |-> "Null",repair_cycle |-> 0,spec_valid |-> "pass"])
    >>
----


=============================================================================

---- CONFIG DispatchTdd_TTrace_1773533780 ----
CONSTANTS
    MaxRepairCycles = 2
    MaxSpecCycles = 2
    Null = "Null"

INVARIANT
    _inv

CHECK_DEADLOCK
    \* CHECK_DEADLOCK off because of PROPERTY or INVARIANT above.
    FALSE

INIT
    _init

NEXT
    _next

CONSTANT
    _TETrace <- _trace

ALIAS
    _expression
=============================================================================
\* Generated on Sun Mar 15 00:16:21 GMT 2026