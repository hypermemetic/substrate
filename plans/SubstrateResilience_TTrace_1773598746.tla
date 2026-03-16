---- MODULE SubstrateResilience_TTrace_1773598746 ----
EXTENDS Sequences, TLCExt, SubstrateResilience, Toolbox, Naturals, TLC

_expression ==
    LET SubstrateResilience_TEExpression == INSTANCE SubstrateResilience_TEExpression
    IN SubstrateResilience_TEExpression!expression
----

_trace ==
    LET SubstrateResilience_TETrace == INSTANCE SubstrateResilience_TETrace
    IN SubstrateResilience_TETrace!trace
----

_inv ==
    ~(
        TLCGet("level") = Len(_TETrace)
        /\
        agent = ([RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE])
        /\
        substrate = ("Up")
        /\
        status = ([RUNPLAN |-> "Failed", WATCHTREE |-> "Pending", RETRY1 |-> "Failed", RETRY2 |-> "Pending"])
    )
----

_init ==
    /\ agent = _TETrace[1].agent
    /\ substrate = _TETrace[1].substrate
    /\ status = _TETrace[1].status
----

_next ==
    /\ \E i,j \in DOMAIN _TETrace:
        /\ \/ /\ j = i + 1
              /\ i = TLCGet("level")
        /\ agent  = _TETrace[i].agent
        /\ agent' = _TETrace[j].agent
        /\ substrate  = _TETrace[i].substrate
        /\ substrate' = _TETrace[j].substrate
        /\ status  = _TETrace[i].status
        /\ status' = _TETrace[j].status

\* Uncomment the ASSUME below to write the states of the error trace
\* to the given file in Json format. Note that you can pass any tuple
\* to `JsonSerialize`. For example, a sub-sequence of _TETrace.
    \* ASSUME
    \*     LET J == INSTANCE Json
    \*         IN J!JsonSerialize("SubstrateResilience_TTrace_1773598746.json", _TETrace)

=============================================================================

 Note that you can extract this module `SubstrateResilience_TEExpression`
  to a dedicated file to reuse `expression` (the module in the 
  dedicated `SubstrateResilience_TEExpression.tla` file takes precedence 
  over the module `SubstrateResilience_TEExpression` below).

---- MODULE SubstrateResilience_TEExpression ----
EXTENDS Sequences, TLCExt, SubstrateResilience, Toolbox, Naturals, TLC

expression == 
    [
        \* To hide variables of the `SubstrateResilience` spec from the error trace,
        \* remove the variables below.  The trace will be written in the order
        \* of the fields of this record.
        agent |-> agent
        ,substrate |-> substrate
        ,status |-> status
        
        \* Put additional constant-, state-, and action-level expressions here:
        \* ,_stateNumber |-> _TEPosition
        \* ,_agentUnchanged |-> agent = agent'
        
        \* Format the `agent` variable as Json value.
        \* ,_agentJson |->
        \*     LET J == INSTANCE Json
        \*     IN J!ToJson(agent)
        
        \* Lastly, you may build expressions over arbitrary sets of states by
        \* leveraging the _TETrace operator.  For example, this is how to
        \* count the number of times a spec variable changed up to the current
        \* state in the trace.
        \* ,_agentModCount |->
        \*     LET F[s \in DOMAIN _TETrace] ==
        \*         IF s = 1 THEN 0
        \*         ELSE IF _TETrace[s].agent # _TETrace[s-1].agent
        \*             THEN 1 + F[s-1] ELSE F[s-1]
        \*     IN F[_TEPosition - 1]
    ]

=============================================================================



Parsing and semantic processing can take forever if the trace below is long.
 In this case, it is advised to uncomment the module below to deserialize the
 trace from a generated binary file.

\*
\*---- MODULE SubstrateResilience_TETrace ----
\*EXTENDS IOUtils, SubstrateResilience, TLC
\*
\*trace == IODeserialize("SubstrateResilience_TTrace_1773598746.bin", TRUE)
\*
\*=============================================================================
\*

---- MODULE SubstrateResilience_TETrace ----
EXTENDS SubstrateResilience, TLC

trace == 
    <<
    ([agent |-> [RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Pending", WATCHTREE |-> "Pending", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Ready", WATCHTREE |-> "Pending", RETRY1 |-> "Pending", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Ready", WATCHTREE |-> "Pending", RETRY1 |-> "Ready", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> TRUE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Running", WATCHTREE |-> "Pending", RETRY1 |-> "Ready", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> TRUE, WATCHTREE |-> FALSE, RETRY1 |-> TRUE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Running", WATCHTREE |-> "Pending", RETRY1 |-> "Running", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> TRUE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Failed", WATCHTREE |-> "Pending", RETRY1 |-> "Running", RETRY2 |-> "Pending"]]),
    ([agent |-> [RUNPLAN |-> FALSE, WATCHTREE |-> FALSE, RETRY1 |-> FALSE, RETRY2 |-> FALSE],substrate |-> "Up",status |-> [RUNPLAN |-> "Failed", WATCHTREE |-> "Pending", RETRY1 |-> "Failed", RETRY2 |-> "Pending"]])
    >>
----


=============================================================================

---- CONFIG SubstrateResilience_TTrace_1773598746 ----
CONSTANTS
    UseRecovery = FALSE
    UseFailPropagation = FALSE

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
\* Generated on Sun Mar 15 18:19:07 GMT 2026