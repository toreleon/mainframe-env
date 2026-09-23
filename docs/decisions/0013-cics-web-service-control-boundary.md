# ADR-0013: Bounded CICS web-service-control authority

Status: **Proposed for v0.9 development; acceptance gate pending**
Owner: **CICS and execution maintainers**
Scope: **Eight typed web-service-control rows, local service transport, and durable channel state**
Applies from: **mainframe-env 0.9.0 development**

The eight CICS application rows 0107, 0197–0199, and 0259–0262 use owned typed
MCEP v2 operation and operand identities. New operation tags are 140–147,
operand tags are within 512–575, and non-ASSIGN output tags are within 568–631.
The assigned option range 444–507 remains reserved because the source syntax
uses value-bearing CVDAs rather than new bare flag options. V1 decoding and
all pre-existing tag identities remain unchanged.

The provider stores SOAP fault and WS-Addressing context on a run-unit/channel
key. State changes and their exact CICS result replay are one atomic provider
state batch. INVOKE SERVICE uses a registered immutable local program generation
as the installed service transport. It reserves dispatch before the nested call;
if the result cannot be sealed after dispatch, retries report UnknownOutcome.
On success, the returned channel container, web state, and effect replay are
atomically written. Memory and SQLite stores use the same contract.

The route validates source option combinations, typed output widths, SOAP and
requester/provider execution context, bounded XML and URI inputs, and supported
UTF-8 or IBM-037 conversion. CICS condition responses and EIB/RESP/RESP2 flow
through the existing typed host and interpreter boundaries. Transaction and
resource SAF decisions are audited before state change. A remote HTTP transport
is outside this bounded local service route; the implementation does not claim
remote transport or licensed IBM equivalence.

The eight exact IBM topic paths and SHA-256 identities are recorded in the
[0.9.0 status](../delivery/coverage-versions/status/0.9.0.md).
