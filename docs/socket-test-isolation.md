# Socket-release fixture isolation

This is Linux test evidence for the raw Unix-socket and UDP rebinding fixtures,
not a new production transport guarantee or cross-platform qualification.

## Observation and controlled case

| Experiment | Observed result |
| --- | --- |
| Original IPC lifecycle case, 100 isolated repetitions | Passed; insufficient to establish full-suite stability |
| Original complete IPC module, 100 concurrent-module repetitions | Passed; the broader harness still reproduced a connection to the supposedly closed replacement listener |
| Frozen full library suite before isolation | Failed on its first repetition: the replacement listener still accepted a connection, rather than its pathname disappearing |
| Controlled duplicate listener description | Closing the original handle retained live connectivity and refused a competing bind while the explicit duplicate remained open; closing the last known description restored stale-socket refusal |
| IPC isolation only, repeated full library suite | IPC remained passing; repetition nine exposed a UDP-rebinding assertion in the transport lifetime fixture |
| IPC and transport socket fixtures isolated, ten full library repetitions | All ten completed with their original refusal, release and rebinding assertions retained |

`CLOEXEC` closes a descriptor at exec, not at fork. Other process fixtures in a
shared harness can therefore temporarily hold copied socket descriptions. The
controlled duplicate demonstrates why dropping one handle is not evidence that
the underlying listener is closed. It is consistent with the observed failure;
this investigation did not trace every concurrent child descriptor individually.

## Test boundary

The parent tests launch exact child test cases. Socket creation occurs after
exec, outside the parent harness's unrelated fork activity. The child cases have
explicit arming flags; an ordinary ignored-test run does not perform them. Parent
wrappers remain active and assert successful child execution. The child test
output confirms execution, not an empty selection.

Each new parent wrapper also imposes an outer child-process deadline. It kills
and reaps a timed-out child, and cancellation requests kill-on-drop. This adds a
process-lifetime bound without widening any original protocol or test deadline.

All original stale-socket, capture-destruction, cancellation, diagnostics and
physical rebinding assertions remain. The controlled duplicate adds coverage;
there is no retry-until-green, added sleep, widened deadline, production guard
change or assertion removal. The release-registry destruction-order test still
runs directly in the concurrent parent harness.

These bounded repetitions are evidence, not an unlimited stability proof.
Same-kernel port allocation can still involve other processes. Earlier quorum
and profile-request timeouts were not reproduced in the final ten runs, but no
causal fix for those separate observations is claimed here. Native results and
exact distributable-byte qualification remain separate gates.
