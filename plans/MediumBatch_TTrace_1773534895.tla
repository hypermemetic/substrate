---- MODULE MediumBatch_TTrace_1773534895 ----
EXTENDS Sequences, TLCExt, MediumBatch, Toolbox, Naturals, TLC

_expression ==
    LET MediumBatch_TEExpression == INSTANCE MediumBatch_TEExpression
    IN MediumBatch_TEExpression!expression
----

_trace ==
    LET MediumBatch_TETrace == INSTANCE MediumBatch_TETrace
    IN MediumBatch_TETrace!trace
----

_inv ==
    ~(
        TLCGet("level") = Len(_TETrace)
        /\
        status = ([RUNPLAN |-> "Running", WATCHTREE |-> "Running", RETRY1 |-> "Pending", RETRY2 |-> "Pending"])
    )
----

_init ==
    /\ status = _TETrace[1].status
----

_next ==
    /\ \E i,j \in DOMAIN _TETrace:
        /\ \/ /\ j = i + 1
              /\ i = TLCGet("level")
        /\ status  = _TETrace[i].status
        /\ status' = _TETrace[j].status

\* Uncomment the ASSUME below to write the states of the error trace
\* to the given file in Json format. Note that you can pass any tuple
\* to `JsonSerialize`. For example, a sub-sequence of _TETrace.
    \* ASSUME
    \*     LET J == INSTANCE Json
    \*         IN J!JsonSerialize("MediumBatch_TTrace_1773534895.json", _TETrace)

=============================================================================

 Note that you can extract this module `MediumBatch_TEExpression`
  to a dedicated file to reuse `expression` (the module in the 
  dedicated `MediumBatch_TEExpression.tla` file takes precedence 
  over the module `MediumBatch_TEExpression` below).

---- MODULE MediumBatch_TEExpression ----
EXTENDS Sequences, TLCExt, MediumBatch, Toolbox, Naturals, TLC

expression == 
    [
        \* To hide variables of the `MediumBatch` spec from the error trace,
        \* remove the variables below.  The trace will be written in the order
        \* of the fields of this record.
        status |-> status
        
        \* Put additional constant-, state-, and action-level expressions here:
        \* ,_stateNumber |-> _TEPosition
        \* ,_statusUnchanged |-> status = status'
        
        \* Format the `status` variable as Json value.
        \* ,_statusJson |->
        \*     LET J == INSTANCE Json
        \*     IN J!ToJson(status)
        
        \* Lastly, you may build expressions over arbitrary sets of states by
        \* leveraging the _TETrace operator.  For example, this is how to
        \* count the number of times a spec variable changed up to the current
        \* state in the trace.
        \* ,_statusModCount |->
        \*     LET F[s \in DOMAIN _TETrace] ==
        \*         IF s = 1 THEN 0
        \*         ELSE IF _TETrace[s].status # _TETrace[s-1].status
        \*             THEN 1 + F[s-1] ELSE F[s-1]
        \*     IN F[_TEPosition - 1]
    ]

=============================================================================



Parsing and semantic processing can take forever if the trace below is long.
 In this case, it is advised to uncomment the module below to deserialize the
 trace from a generated binary file.

\*
\*---- MODULE MediumBatch_TETrace ----
\*EXTENDS IOUtils, MediumBatch, TLC
\*
\*trace == IODeserialize("MediumBatch_TTrace_1773534895.bin", TRUE)
\*
\*=============================================================================
\*

---- MODULE MediumBatch_TETrace ----
EXTENDS MediumBatch, TLC

trace == 
    <<
    ([status |-> [RUNPLAN |-> "Pending", WATCHTREE |-> "Pending", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([status |-> [RUNPLAN |-> "Ready", WATCHTREE |-> "Pending", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([status |-> [RUNPLAN |-> "Ready", WATCHTREE |-> "Ready", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([status |-> [RUNPLAN |-> "Running", WATCHTREE |-> "Ready", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([status |-> [RUNPLAN |-> "Running", WATCHTREE |-> "Running", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]])
    >>
----


=============================================================================

---- CONFIG MediumBatch_TTrace_1773534895 ----
CONSTANTS
    UseFileLocks = FALSE

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
\* Generated on Sun Mar 15 00:34:56 GMT 2026