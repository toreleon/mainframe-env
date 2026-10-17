# ADR 0070: compiled CICS first protected dispatch

Status: Proposed; source-only draft, not integrated or executed.
Owner: CICS task, compiled program router and coordinator/store boundary maintainers
Scope: one validated installed initial CICS machine and its first protected external dispatch
Applies from: mainframe-env opt-in compiled CICS first protected dispatch

The ordinary terminal route constructs its machine from the installed executable and drives it with a coordinator. A cloned terminal context does not retain the service's command claim, a CALL receipt does not admit a new protected root, and a provider-state row with matching bytes does not identify the same physical store.

The opt-in boundary joins the actual validated ArtifactStore record and exact online-program row to the machine it constructs. A fresh coordinator root is admitted through the existing physical store before the first drive. The real initial terminal CommandLease reconstructs and compares the whole initial actor, checks the installed definition and artifact, and borrows the current task while the embedding admits its frame. Preparation happens once after that admission.

At the first non-CICS external effect, the existing durable dispatch helper produces the actual Intent and invokes the original ScopedHostService at most once. The real current-program CommandLease is held while the embedding receives borrowed compiled-program, storage-root, Intent, and task observations. Same physical store and artifact-adapter identity are required; equal identifiers or copied rows do not substitute. Every refusal or return at this first-dispatch boundary retains UnknownOutcome before task release. A callback panic uses the original loan fence. The coordinator then fences its original root and retains the actual effect outcome. No second machine drive or terminal settlement is admitted.

All ordinary terminal, program, native-root and provider routes keep their defaults. The new host port defaults to Unsupported and cannot return a replacement host result, create an attestation, settle an MQ unit of work, or change a connection/descriptor version. Existing root scope, actor, operation, byte, cancellation and deadline bounds remain unchanged. The optional MQ frame is still validated by the existing machine ABI restrictions.

The draft's compiled non-MQ control proves local infrastructure only. The pinned CardDemo CODATE01 source first offers a CICS MQOPEN with its original implicit connection and MQOD shape. The current typed MQ profile admits z/OS batch queue-manager owners and its existing structures; this draft does not expand it. Pinned IBM MQ 9.4 bodies and licensed captures remain unavailable for the semantic boundary. No CardDemo MQ workflow, reply, cold reconnect, pending PUT, GET, unit-of-work, vendor, or Foundation completion credit follows from this component.

Integration must compose this proposed record into the actual current decision registry and navigation after ADRs 0063–0069. The parent metadata is not copied by this draft.
