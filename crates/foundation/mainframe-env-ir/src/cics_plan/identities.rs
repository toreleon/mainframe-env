mod option;
mod output;
pub use option::CicsPlanOption;
pub use output::CicsOutputName;

/// CICS operation selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOperation {
    /// BTS process and activity lifecycle commands, catalog rows 0002–0217.
    AcquireActivityId,
    AcquireProcess,
    CancelAcqActivity,
    CancelAcqProcess,
    CancelActivity,
    CheckAcqActivity,
    CheckAcqProcess,
    CheckActivity,
    DefineActivity,
    DefineProcess,
    DeleteActivity,
    ResetAcqProcess,
    ResetActivity,
    ResumeAcqActivity,
    ResumeAcqProcess,
    ResumeActivity,
    RunAcqActivity,
    RunAcqProcess,
    RunActivity,
    RunTransId,
    SuspendAcqActivity,
    SuspendAcqProcess,
    SuspendActivity,
    /// Fetch one eligible parent-owned child task.
    FetchAny,
    /// Fetch one child task by its opaque token.
    FetchChild,
    /// Release one parent-owned child token.
    FreeChild,
    /// Link to the UOW-acquired BTS activity.
    LinkAcqActivity,
    /// Link to the UOW-acquired BTS process root.
    LinkAcqProcess,
    /// Link to a named child of the current activity.
    LinkActivity,
    /// Read one owned LUTYPE6.1 or MRO attach header.
    ExtractAttach,
    /// Read mapped APPC or MRO state.
    ExtractAttributes,
    /// Read basic APPC state with GDS return code.
    GdsExtractAttributes,
    /// Extract one terminal logon message.
    ExtractLogonMsg,
    /// Read mapped APPC attach process information.
    ExtractProcess,
    /// Read basic APPC attach process information.
    GdsExtractProcess,
    /// Translate an LUTYPE6.1 network name to local IDs.
    ExtractTct,
    /// Position on one owned conversation facility.
    Point,
    /// Allocate a mapped APPC or MRO task-owned conversation.
    AllocateConversation,
    /// Allocate an APPC basic conversation and return a GDS code.
    GdsAllocateConversation,
    /// Return the principal APPC basic conversation identity.
    GdsAssignConversation,
    /// Construct one task-local MRO or LU6.1 attach header.
    BuildAttach,
    /// Connect an allocated APPC mapped conversation to a process.
    ConnectProcess,
    /// Connect an allocated APPC basic conversation to a process.
    GdsConnectProcess,
    /// Send and receive through one mapped APPC or MRO conversation.
    Converse,
    /// Return one mapped APPC or MRO session to CICS.
    FreeConversation,
    /// Return an APPC basic session after the peer reaches FREE.
    GdsFreeConversation,
    ReceiveConversation,
    GdsReceiveConversation,
    SendConversation,
    GdsWaitConversation,
    WaitConvid,
    WaitSignal,
    WaitTerminal,
    /// Change a standard RACF password under one SAF effect.
    ChangePassword,
    /// Change a length-selected password or phrase under one SAF effect.
    ChangePhrase,
    /// Add an atomic child to a BTS composite event.
    AddSubevent,
    /// Terminate the issuing task abnormally or transfer to its active exit.
    Abend,
    /// Return checked virtual addresses for task storage areas.
    Address,
    /// Copy one checked virtual pointer/address relationship.
    AddressSet,
    /// Refresh the EIB clock fields and return one absolute-time value.
    Asktime,
    /// Refresh the implicit EIB date and time fields.
    AsktimeEib,
    /// Transform one absolute-time value into selected display/binary fields.
    FormatTime,
    /// Convert one architected date-time string into CICS absolute time.
    ConvertTime,
    /// Write one operator console message and optionally await its reply.
    WriteOperator,
    /// Extract selected fields of the accepted TCP/IP client certificate.
    ExtractCertificate,
    /// Extract source-bounded accepted TCP/IP connection fields.
    ExtractTcpip,
    /// Remove editing characters from one EBCDIC numeric field in place.
    BifDeedit,
    /// Compute a source-bounded SHA-1 digest in one of three representations.
    BifDigest,
    /// Release one task-local virtual storage area acquired by GETMAIN.
    Freemain,
    /// Release one checked AMODE(64) virtual allocation.
    Freemain64,
    /// Allocate one bounded task-local virtual storage area.
    Getmain,
    /// Allocate a checked AMODE(64) virtual allocation.
    Getmain64,
    /// Change the issuing task's dispatch priority and optionally yield.
    ChangeTask,
    /// Cancel one unhonored local interval-control START request.
    Cancel,
    /// Complete a source-defined zero-delay request without suspension.
    Delay,
    /// Define one signed fullword named counter.
    DefineCounter,
    /// Define one unsigned doubleword named counter.
    DefineDCounter,
    /// IBM DELETE named-counter command.
    DeleteCounter,
    /// IBM DELETE named-counter command.
    DeleteDCounter,
    /// IBM GET named-counter command.
    GetCounter,
    /// IBM GET named-counter command.
    GetDCounter,
    /// IBM QUERY named-counter command.
    QueryCounter,
    /// IBM QUERY named-counter command.
    QueryDCounter,
    /// IBM REWIND named-counter command.
    RewindCounter,
    /// IBM REWIND named-counter command.
    RewindDCounter,
    /// IBM UPDATE named-counter command.
    UpdateCounter,
    /// IBM UPDATE named-counter command.
    UpdateDCounter,
    /// Arm one task-owned timer-event control area for later posting.
    Post,
    /// Release one task-owned enqueue.
    Deq,
    /// Acquire one task-owned enqueue.
    Enq,
    /// Install or deactivate one bounded set of terminal AID handlers.
    HandleAid,
    /// Activate, cancel, or reactivate one abnormal-termination exit.
    HandleAbend,
    /// Install or deactivate one bounded set of reviewed condition handlers.
    HandleCondition,
    /// Ignore one bounded set of reviewed EIBRESP conditions.
    IgnoreCondition,
    /// Invoke one installed program at the next logical level and return.
    Link,
    /// Select and invoke one installed application operation at the next logical level.
    InvokeApplication,
    /// Make one immutable installed program generation available to the issuing task.
    Load,
    /// Release one prior program LOAD ownership level.
    Release,
    /// Transfer to one installed program at the same logical level without returning.
    Xctl,
    /// Return from the current top-level task and optionally schedule its next transaction.
    Return,
    /// Position one default-key file browse without reading a record.
    StartBrowse,
    /// Reposition an active file browse while retaining its cursor identity.
    ResetBrowse,
    /// Read the next record in one default-key file browse.
    ReadNext,
    /// Read the previous record in one default-key file browse.
    ReadPrev,
    /// End one default-key file browse.
    EndBrowse,
    /// Restore one suspended HANDLE/IGNORE specification snapshot.
    PopHandle,
    /// Suspend the current HANDLE/IGNORE specifications in one nested snapshot.
    PushHandle,
    /// Read one file record.
    Read,
    /// Delete an explicitly identified or currently held file record.
    Delete,
    /// Define one input event in the current BTS activity.
    DefineInputEvent,
    /// Define a BTS composite event with an AND or OR predicate.
    DefineCompositeEvent,
    /// Delete one BTS input or composite event.
    DeleteEvent,
    /// BTS timer state command.
    CheckTimer,
    /// BTS timer state command.
    DefineTimer,
    /// BTS timer state command.
    DeleteTimer,
    /// BTS event retrieval or status command.
    RetrieveReattachEvent,
    /// BTS event retrieval or status command.
    RetrieveSubevent,
    /// BTS event retrieval or status command.
    TestEvent,
    /// Emit matching business events from an application capture point.
    SignalEvent,
    /// BTS timer state command.
    ForceTimer,
    /// Write one explicitly keyed file record.
    Write,
    /// Write one bounded record to a transient data queue.
    WriteTransientData,
    /// Delete every record from one local transient-data queue.
    DeleteTransientData,
    /// Delete every item from one local temporary-storage queue.
    DeleteTemporaryStorage,
    /// Read and consume one record from a local transient-data queue.
    ReadTransientData,
    /// Remove an atomic child from a BTS composite event.
    RemoveSubevent,
    /// Read one item from a local temporary-storage queue.
    ReadTemporaryStorage,
    /// Append or replace one item in a local temporary-storage queue.
    WriteTemporaryStorage,
    /// Receive one mapped terminal input message.
    ReceiveMap,
    /// Receive data and a partition name from the active 8775 set.
    ReceivePartn,
    /// Send one mapped terminal output message.
    SendMap,
    /// Send one unmapped terminal text message.
    SendText,
    /// Associate a registered BMS partition set with the issuing task.
    SendPartnset,
    /// Send device controls to the terminal or active BMS logical message.
    SendControl,
    /// Outboard batch data interchange abort operation.
    IssueAbort,
    /// Outboard batch data interchange add operation.
    IssueAdd,
    /// Outboard batch data interchange end operation.
    IssueEnd,
    /// Outboard batch data interchange erase operation.
    IssueErase,
    /// Outboard batch data interchange note operation.
    IssueNote,
    /// Outboard batch data interchange query operation.
    IssueQuery,
    /// Outboard batch data interchange receive operation.
    IssueReceive,
    /// Outboard batch data interchange replace operation.
    IssueReplace,
    /// Outboard batch data interchange send operation.
    IssueSend,
    /// Outboard batch data interchange wait operation.
    Route,
    IssueWait,
    /// Complete a full-BMS logical message and dispatch its final page.
    SendPage,
    /// Rewrite the record held by the current update context.
    Rewrite,
    /// Commit or roll back the current unit of work.
    Syncpoint,
    /// Invalidate a task-owned file update token or no-token hold.
    Unlock,
    /// Overwrite the originating task's bounded user correlator data.
    SetAssociationUserCorrData,
    /// Close one task-owned spool report.
    SpoolClose,
    /// Open one matching spool report for input.
    SpoolOpenInput,
    /// Create one spool report for output.
    SpoolOpenOutput,
    /// Read the next record of an input spool report.
    SpoolRead,
    /// Append one record to an output spool report.
    SpoolWrite,
    /// Write one numeric user trace entry to the active local destinations.
    EnterTraceNum,
    /// Apply one installed local user event monitoring definition.
    Monitor,
    /// Capture selected local transaction data and diagnostic table content.
    DumpTransaction,
    /// Capture selected local diagnostic state through the DUMP form.
    Dump,
    /// Set bounded local trace-control switches.
    Trace,
    /// Retain one named local diagnostic trace event.
    EnterTraceId,
    /// Yield the issuing task once for redispatch.
    Suspend,
    /// Wait for one timer-event control area to be posted.
    WaitEvent,
    /// Wait for standard MVS posting of one ECB in a bounded external list.
    WaitExternal,
    /// Wait on one or more MVS-format ECBs, including hand-posted events.
    WaitCics,
    /// Return one bounded set of task, terminal, and invocation context values.
    Assign,
    /// Query source-defined SAF access levels for a CICS or named resource.
    QuerySecurity,
    /// Issue a one-use PassTicket for the current task principal.
    RequestPassTicket,
    /// Issue an encrypted PassTicket using a task-scoped token key.
    RequestEncryptPassTicket,
    /// Authenticate a user and bind that identity to the terminal.
    Signon,
    /// Clear the terminal's signed-on identity for future tasks.
    Signoff,
    /// Verify a standard password through the installed SAF authority.
    VerifyPassword,
    /// Verify a password or phrase selected by its explicit length.
    VerifyPhrase,
    /// Verify one bounded token through the installed SAF authority.
    VerifyToken,
    /// Discard the current full-BMS logical message, if one is being built.
    PurgeMessage,
    /// Schedule one local interval-control START data record.
    Start,
    /// Start one noncancelable facility-less local task immediately.
    StartAttach,
    /// Start one local transaction under a selected 3270 bridge exit.
    StartBrexit,
    /// Consume one expired interval-control START data record.
    Retrieve,
    /// Typed CICS web-service-control operation InvokeService.
    InvokeService,
    /// Typed CICS web-service-control operation SoapFaultAdd.
    SoapFaultAdd,
    /// Typed CICS web-service-control operation SoapFaultCreate.
    SoapFaultCreate,
    /// Typed CICS web-service-control operation SoapFaultDelete.
    SoapFaultDelete,
    /// Typed CICS web-service-control operation WsaContextBuild.
    WsaContextBuild,
    /// Typed CICS web-service-control operation WsaContextDelete.
    WsaContextDelete,
    /// Typed CICS web-service-control operation WsaContextGet.
    WsaContextGet,
    /// Typed CICS web-service-control operation WsaEprCreate.
    WsaEprCreate,
    /// Create one bounded transaction-owned document.
    DocumentCreate,
    /// Delete one transaction-owned document and release its storage.
    DocumentDelete,
    /// Insert content or bookmarks into one transaction-owned document.
    DocumentInsert,
    /// Copy one transaction-owned document into an application buffer.
    DocumentRetrieve,
    /// Add or replace symbols in one transaction-owned document.
    DocumentSet,
    /// Convert one BIT-mode application-data container to canonical JSON.
    TransformDataToJson,
    /// Convert one BIT-mode application-data container to deterministic XML.
    TransformDataToXml,
    /// Convert one JSON container to BIT-mode application data.
    TransformJsonToData,
    /// Query XML metadata or convert XML to BIT-mode application data.
    TransformXmlToData,
    /// Parse one bounded URL without opening a web session.
    WebParseUrl,
    /// Open one bounded task-owned HTTP client connection.
    WebOpen,
    /// Close one task-owned HTTP client session.
    WebClose,
    /// Extract metadata from one inbound request or client session.
    WebExtract,
    /// EXTRACT WEB spelling of the checked Web metadata command.
    ExtractWeb,
    /// Read one bounded HTTP header, query parameter, or form field.
    WebRead,
    /// Start one task-owned Web header, query, or form browse.
    WebStartBrowse,
    /// Read and advance one task-owned Web browse cursor.
    WebReadNext,
    /// End one task-owned Web browse.
    WebEndBrowse,
    /// Stage one HTTP header for the next Web message.
    WebWrite,
    /// Send one checked Web client request or server response.
    WebSend,
    /// Return the pending server WEB SEND document token.
    WebRetrieve,
    /// Consume a bounded HTTP body for a server request or client response.
    WebReceive,
    /// Dispatch one client request and receive its bounded response.
    WebConverse,
    /// Synchronize the issuing task with one named journal output request.
    WaitJournalName,
    /// Synchronize the issuing task with output for one numbered journal.
    WaitJournalNum,
    /// Create one named journal record for synchronous or deferred output.
    WriteJournalName,
    /// Create one numbered journal record for synchronous or deferred output.
    WriteJournalNum,
}

/// Named input accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOperandName {
    BtsActivityId,
    BtsProcess,
    BtsProcessType,
    BtsActivity,
    BtsEvent,
    BtsInputEvent,
    BtsTransId,
    BtsProgram,
    BtsUserId,
    BtsFacilityToken,
    BtsChannel,
    /// Opaque sixteen-byte child token.
    BtsChild,
    /// Name of a current activity's child.
    BtsLinkActivity,
    /// Input event for a dormant BTS activity.
    BtsLinkInputEvent,
    /// Fullword wait limit in milliseconds.
    BtsTimeout,
    /// Task-local attach-header identifier.
    ConversationAttachId,
    /// Four-byte conversation token.
    ConversationConvid,
    /// One to four character session identifier.
    ConversationSession,
    /// Process-name receive capacity, defaulting to 32.
    ConversationMaxProcLen,
    /// Eight-character SNA network name.
    ConversationNetName,
    ConversationSysid,
    ConversationPartner,
    ConversationProfile,
    ConversationModeName,
    ConversationProcess,
    ConversationResource,
    ConversationReturnProcess,
    ConversationReturnResource,
    ConversationQueue,
    ConversationIuType,
    ConversationDataStream,
    ConversationRecordFormat,
    ConversationProcName,
    ConversationProcLength,
    ConversationPipList,
    ConversationPipLength,
    ConversationSyncLevel,
    ConversationFrom,
    ConversationFromLength,
    ConversationFromFullLength,
    ConversationMaxLength,
    ConversationMaxFullLength,
    ConversationToLength,
    ConversationToFullLength,
    ConversationDataConvid,
    ConversationDataSession,
    ConversationDataFrom,
    ConversationDataLength,
    ConversationDataFullLength,
    ConversationDataMaxLength,
    ConversationDataMaxFullLength,
    ConversationDataAttachId,
    /// Security resource class supplied to QUERY SECURITY.
    ResClass,
    /// Security resource identifier supplied to QUERY SECURITY.
    ResId,
    /// Fullword length of RESID in a custom-class query.
    ResIdLength,
    /// CICS resource type supplied to QUERY SECURITY.
    ResType,
    /// QUERY SECURITY violation-message selection.
    LogMessage,
    /// User ID supplied to a CICS security-control command.
    SecurityUserId,
    /// Optional group ID for credential verification.
    SecurityGroupId,
    /// Resolved standard password storage; literals are forbidden.
    SecurityPassword,
    /// New password bytes read from COBOL storage.
    SecurityNewPassword,
    /// Proposed password or phrase bytes read from COBOL storage.
    SecurityNewPhrase,
    /// Fullword length of the proposed phrase data.
    SecurityNewPhraseLen,
    /// Resolved phrase storage; literals are forbidden.
    SecurityPhrase,
    /// Fullword length of the supplied phrase data.
    SecurityPhraseLen,
    /// Destination ESM application profile for a PassTicket.
    SecurityEsmAppName,
    /// Three-character requested terminal national language.
    SecurityLanguageCode,
    /// One-character requested terminal national language.
    SecurityNatLang,
    /// Optional terminal card reader credential area.
    SecurityOidCard,
    /// Bounded token bytes and explicit token length.
    SecurityTokenData,
    SecurityTokenLength,
    /// Four-byte same-task key handle from VERIFY TOKEN.
    SecurityEncryptKey,
    /// Name of a BTS event or SIGNAL EVENT capture point.
    Event,
    /// One atomic child of a BTS composite predicate.
    SubEvent,
    /// Initial atomic child of a composite event, numbered one through eight.
    SubEvent1,
    SubEvent2,
    SubEvent3,
    SubEvent4,
    SubEvent5,
    SubEvent6,
    SubEvent7,
    SubEvent8,
    /// SIGNAL EVENT capture data area.
    SignalFrom,
    /// SIGNAL EVENT captured byte length.
    SignalFromLength,
    /// SIGNAL EVENT source channel.
    SignalFromChannel,
    Timer,
    TimerDays,
    TimerHours,
    TimerMinutes,
    TimerSeconds,
    TimerYear,
    TimerMonth,
    TimerDayOfMonth,
    TimerDayOfYear,
    /// Optional application transaction abend code.
    Abcode,
    /// Source label used by a task-local control transfer.
    Label,
    /// Program name selected for a program-control transfer.
    Program,
    /// `COMMAREA(...)` input-output data area.
    Commarea,
    /// `TRANSID(...)` next-transaction name.
    TransId,
    /// `BREXIT(...)` override for a transaction's bridge exit default.
    BrExit,
    /// `BRDATA(...)` initial data passed to the bridge exit.
    BrData,
    /// `BRDATALENGTH(...)` selected initial data length.
    BrDataLength,
    /// `TERMID(...)` principal facility for a started task.
    TermId,
    /// `RTRANSID(...)` metadata passed to a started task.
    ReturnTransId,
    /// `RTERMID(...)` metadata passed to a started task.
    ReturnTermId,
    /// `FILE(...)` resource binding.
    File,
    /// `DATASET(...)` resource alias.
    Dataset,
    /// `FROM(...)` record bytes.
    From,
    /// `RIDFLD(...)` record identifier.
    Ridfld,
    /// `TOKEN(...)` fullword update identifier supplied to UNLOCK/REWRITE/DELETE.
    Token,
    /// `QUEUE(...)` transient-data or temporary-storage resource.
    Queue,
    /// `QNAME(...)` long temporary-storage resource.
    Qname,
    /// `SYSID(...)` target CICS system identity.
    SysId,
    /// `ITEM(...)` temporary-storage item number.
    Item,
    /// `ADDRESS COMMAREA(pointer-reference)` output target.
    CommareaPointer,
    /// `MAP(...)` BMS map name.
    Map,
    /// `MAPSET(...)` BMS mapset name.
    Mapset,
    /// `SEND PARTNSET(name)` partition-set resource name.
    Partnset,
    /// Halfword cursor position for SEND CONTROL.
    ControlCursor,
    /// Four-byte 8775 magnetic stripe reader control value.
    Msr,
    /// Name of the 8775 output partition.
    Outpartn,
    /// Name of the 8775 partition to activate.
    Actpartn,
    /// Logical device code mnemonic.
    Ldc,
    /// Formatted trailer bytes on the last BMS page.
    Trailer,
    /// Outboard 3650 formatting map name.
    Fmhparm,
    /// Outboard DestId input.
    DestId,
    /// Outboard DestIdLength input.
    DestIdLength,
    /// Outboard Subaddress input.
    Subaddress,
    /// Outboard Volume input.
    Volume,
    /// Outboard VolumeLength input.
    VolumeLength,
    /// Outboard NumRec input.
    NumRec,
    /// Outboard KeyNumber input.
    /// Terminal notified for an undeliverable routed message.
    Errterm,
    /// Bounded BMS route title.
    RouteTitle,
    /// Local fixed-width list of terminal or operator targets.
    RouteList,
    /// Three-byte operator-class bit mask.
    Opclass,
    KeyNumber,
    /// `RESOURCE(...)` enqueue identity.
    Resource,
    /// `LENGTH(...)` data length.
    Length,
    /// `DATALENGTH(...)` remote LINK transfer optimization length.
    DataLength,
    /// `MAXLIFETIME(...)` dynamic CVDA value.
    MaxLifetime,
    /// `PRIORITY(...)` task dispatch value.
    Priority,
    /// `USERCORRDATA(...)` task association value.
    UserCorrData,
    /// `SET(ADDRESS OF data-area)` target.
    SetAddress,
    /// `SET(pointer-reference)` target.
    SetPointer,
    /// `USING(ADDRESS OF data-area)` source.
    UsingAddress,
    /// `USING(pointer-reference)` source.
    UsingPointer,
    /// Canonical EIBRESP condition specifications for HANDLE or IGNORE.
    Conditions,
    /// Canonical terminal AID handler specifications.
    Aids,
    /// `ABSTIME(...)` packed-decimal input.
    Abstime,
    /// `DATESTRING(...)` architected date-time input.
    DateString,
    /// `FIELD(...)` in-place built-in DEEDIT source.
    Field,
    /// `RECORD(...)` source for BIF DIGEST.
    Record,
    /// `RECORDLEN(...)` source byte count for BIF DIGEST.
    RecordLength,
    /// `DIGESTTYPE(...)` named CVDA selector for BIF DIGEST.
    DigestType,
    /// `TEXT(...)` bytes sent to the system console.
    OperatorText,
    /// `TEXTLENGTH(...)` selected text byte count.
    OperatorTextLength,
    /// `ROUTECODES(...)` one-byte console route codes.
    OperatorRouteCodes,
    /// `NUMROUTES(...)` selected route count.
    OperatorNumRoutes,
    /// `CONSNAME(...)` specific system console name.
    OperatorConsName,
    /// `ACTION(...)` retained descriptor code.
    OperatorAction,
    /// `MAXLENGTH(...)` reply area capacity.
    OperatorMaxLength,
    /// `TIMEOUT(...)` reply deadline in seconds.
    OperatorTimeout,
    /// Optional one-byte date separator.
    DateSep,
    /// Optional one-byte time separator.
    TimeSep,
    /// `KEYLENGTH(...)` file key length.
    KeyLength,
    /// `REQID(...)` interval-control request identity.
    ReqId,
    /// Packed `INTERVAL(...)` relative expiration.
    Interval,
    /// Packed `TIME(...)` absolute expiration.
    StartTime,
    UserId,
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
    /// `FLENGTH(...)` fullword allocation length.
    Flength,
    /// AMODE(64) fullword allocation length.
    Flength64,
    /// AMODE(64) DSA location CVDA.
    Location64,
    /// Checked non-LE AMODE(64) caller ABI marker.
    Abi64,
    /// `INITIMG(...)` one-byte initialization image.
    InitImage,
    /// `DATAPOINTER(...)` virtual storage pointer returned by GETMAIN.
    DataPointer,
    /// Eight-byte `DATAPOINTER(...)` value from the AMODE(64) arena.
    DataPointer64,
    /// `DATA(...)` area whose virtual address identifies GETMAIN storage.
    DataArea,
    /// Relocatable `DATA(...)` area bound to one AMODE(64) allocation.
    DataArea64,
    /// `ECADDR(...)` pointer to one timer-event control area.
    EventControlAddress,
    /// Optional `NAME(...)` reason associated with an event wait.
    WaitName,
    /// `ECBLIST(...)` pointer to a list of 31-bit ECB addresses.
    EcbList,
    /// `NUMEVENTS(...)` fullword number of ECB addresses.
    NumEvents,
    /// `PURGEABILITY(...)` CVDA value.
    Purgeability,
    /// `APPLICATION(...)` installed application name.
    Application,
    /// `PLATFORM(...)` installed platform name.
    Platform,
    /// `OPERATION(...)` application entry-point operation name.
    ApplicationOperation,
    /// `MAJORVERSION(...)` application major version.
    MajorVersion,
    /// `MINORVERSION(...)` application minor version.
    MinorVersion,
    /// `CHANNEL(...)` application invocation or transform channel name.
    Channel,
    /// `SET(...)` pointer target for LOAD.
    LoadSet,
    /// `ENTRY(...)` pointer target for LOAD.
    Entry,
    /// `LENGTH(...)` halfword output target for LOAD.
    LoadLength,
    /// `FLENGTH(...)` fullword output target for LOAD.
    LoadFlength,
    /// `DOCTOKEN(...)` identifies one transaction-owned document.
    DocumentToken,
    /// `TEXT(...)` bytes marked for client-code-page conversion.
    Text,
    /// `BINARY(...)` bytes retained without client-code-page conversion.
    Binary,
    /// `FROMDOC(...)` identifies a source document.
    FromDocument,
    /// `TEMPLATE(...)` names a registered document template.
    Template,
    /// `SYMBOLLIST(...)` supplies document symbol definitions.
    SymbolList,
    /// `LISTLENGTH(...)` bounds a symbol-list input.
    ListLength,
    /// `DELIMITER(...)` selects the symbol-list separator.
    Delimiter,
    /// `HOSTCODEPAGE(...)` names the source EBCDIC CCSID.
    HostCodePage,
    /// `BOOKMARK(...)` names a bookmark to create.
    Bookmark,
    /// `SYMBOL(...)` names one document symbol.
    Symbol,
    /// `AT(...)` names the insertion start bookmark.
    AtBookmark,
    /// `TO(...)` names the overlay end bookmark.
    ToBookmark,
    /// `MAXLENGTH(...)` bounds a document retrieval destination.
    MaximumLength,
    /// `CHARACTERSET(...)` names a retrieval target encoding.
    CharacterSet,
    /// `VALUE(...)` supplies one document symbol value.
    SymbolValue,
    /// Source-reviewed web-service-control operand Service.
    Service,
    /// Source-reviewed web-service-control operand ServiceOperation.
    ServiceOperation,
    /// Source-reviewed web-service-control operand Uri.
    Uri,
    /// Source-reviewed web-service-control operand UriMap.
    UriMap,
    /// Source-reviewed web-service-control operand Scope.
    Scope,
    /// Source-reviewed web-service-control operand ScopeLen.
    ScopeLen,
    /// Source-reviewed web-service-control operand FaultCode.
    FaultCode,
    /// Source-reviewed web-service-control operand FaultCodeStr.
    FaultCodeStr,
    /// Source-reviewed web-service-control operand FaultCodeLen.
    FaultCodeLen,
    /// Source-reviewed web-service-control operand FaultString.
    FaultString,
    /// Source-reviewed web-service-control operand FaultStrLen.
    FaultStrLen,
    /// Source-reviewed web-service-control operand NatLang.
    NatLang,
    /// Source-reviewed web-service-control operand SoapRole.
    SoapRole,
    /// Source-reviewed web-service-control operand RoleLength.
    RoleLength,
    /// Source-reviewed web-service-control operand FaultActor.
    FaultActor,
    /// Source-reviewed web-service-control operand FaultActLen.
    FaultActLen,
    /// Source-reviewed web-service-control operand Detail.
    Detail,
    /// Source-reviewed web-service-control operand DetailLength.
    DetailLength,
    /// Source-reviewed web-service-control operand FromCcsid.
    FromCcsid,
    /// Source-reviewed web-service-control operand SubcodeStr.
    SubcodeStr,
    /// Source-reviewed web-service-control operand SubcodeLen.
    SubcodeLen,
    /// Source-reviewed web-service-control operand ContextType.
    ContextType,
    /// Source-reviewed web-service-control operand Action.
    Action,
    /// Source-reviewed web-service-control operand MessageId.
    MessageId,
    /// Source-reviewed web-service-control operand RelatesUri.
    RelatesUri,
    /// Source-reviewed web-service-control operand RelatesType.
    RelatesType,
    /// Source-reviewed web-service-control operand RelatesIndex.
    RelatesIndex,
    /// Source-reviewed web-service-control operand EprType.
    EprType,
    /// Source-reviewed web-service-control operand EprField.
    EprField,
    /// Source-reviewed web-service-control operand EprFrom.
    EprFrom,
    /// Source-reviewed web-service-control operand EprLength.
    EprLength,
    /// Source-reviewed web-service-control operand FromCodepage.
    FromCodepage,
    /// Source-reviewed web-service-control operand IntoCcsid.
    IntoCcsid,
    /// Source-reviewed web-service-control operand IntoCodepage.
    IntoCodepage,
    /// Source-reviewed web-service-control operand Address.
    Address,
    /// Source-reviewed web-service-control operand RefParms.
    RefParms,
    /// Source-reviewed web-service-control operand RefParmsLen.
    RefParmsLen,
    /// Source-reviewed web-service-control operand Metadata.
    Metadata,
    /// Source-reviewed web-service-control operand MetadataLen.
    MetadataLen,
    /// Transform input-container name.
    InContainer,
    /// Transform output-container name.
    OutContainer,
    /// JSON transformer resource name.
    Transformer,
    /// Application-data container used by XML transformation.
    DataContainer,
    /// XML container used by XML transformation.
    XmlContainer,
    /// XML transformer resource name.
    XmlTransform,
    /// XML namespace-declarations container.
    NsContainer,
    /// XML element local-name input/output storage.
    ElementName,
    /// XML element namespace input/output storage.
    ElementNamespace,
    /// XML type local-name input/output storage.
    TypeName,
    /// XML type namespace input/output storage.
    TypeNamespace,
    /// Element-name buffer length.
    ElementNameLength,
    /// Element-namespace buffer length.
    ElementNamespaceLength,
    /// Type-name buffer length.
    TypeNameLength,
    /// Type-namespace buffer length.
    TypeNamespaceLength,
    /// `COUNTER(...)` or `DCOUNTER(...)` named-counter identity.
    CounterName,
    /// `POOL(...)` named-counter pool selector.
    CounterPool,
    /// `VALUE(...)` initial named-counter value.
    CounterValue,
    /// `MINIMUM(...)` named-counter lower bound.
    CounterMinimum,
    /// `MAXIMUM(...)` named-counter upper bound.
    CounterMaximum,
    /// `INCREMENT(...)` GET or REWIND reservation size.
    CounterIncrement,
    /// `COMPAREMIN(...)` inclusive lower comparison.
    CounterCompareMin,
    /// `COMPAREMAX(...)` inclusive upper comparison.
    CounterCompareMax,
    /// `JOURNALNAME(...)` named journal identity.
    JournalName,
    /// `JOURNALNUM(...)` numeric journal identity from 1 to 99.
    JournalNum,
    /// `JTYPEID(...)` two-character record origin.
    JournalTypeId,
    /// `FROM(...)` journal record data area.
    JournalFrom,
    /// `FLENGTH(...)` fullword journal data length.
    JournalFlength,
    /// `PREFIX(...)` journal record prefix data area.
    JournalPrefix,
    /// `PFXLENG(...)` halfword journal prefix length.
    JournalPfxLeng,
    /// `REQID(...)` fullword token in the journal-control identity domain.
    JournalReqId,
    /// Eight-byte CICS spool report token.
    SpoolToken,
    /// Spool external-writer or destination user identity.
    SpoolUserId,
    /// One-character spool class.
    SpoolClass,
    /// Destination node for a spool output report.
    SpoolNode,
    /// Maximum output record length.
    SpoolRecordLength,
    /// Double-indirect OUTPUT descriptor pointer.
    SpoolOutDescr,
    /// Maximum SPOOLREAD transfer length.
    SpoolMaxFlength,
    /// SPOOLWRITE record source.
    SpoolFrom,
    TraceNum,
    TraceFrom,
    TraceFromLength,
    TraceResource,
    MonitorPoint,
    MonitorEntryName,
    MonitorData1,
    MonitorData2,
    DumpCode,
    DumpFrom,
    DumpLength,
    DumpFlength,
    DumpSegmentList,
    DumpLengthList,
    DumpNumSegments,
    TraceId,
    TraceIdFrom,
    TraceIdResource,
    TraceEntryName,
    /// Optional SPOOLWRITE transfer length.
    SpoolFlength,
    /// Complete URL supplied to WEB PARSE URL.
    WebUrl,
    /// Fullword length of the supplied URL.
    WebUrlLength,
    /// Fullword input capacity for the returned host.
    WebHostLength,
    /// Fullword input capacity for the returned path.
    WebPathLength,
    /// Fullword input capacity for the returned query string.
    WebQueryStringLength,
    /// Direct WEB OPEN host name.
    WebHost,
    /// Direct WEB OPEN TCP port.
    WebPortNumber,
    /// WEB OPEN HTTP or HTTPS scheme.
    WebScheme,
    /// Installed client URIMAP name.
    WebUriMap,
    /// TLS client certificate label.
    WebCertificate,
    /// Host-side connection code page.
    WebCodePage,
    /// Eight-byte client session token supplied to WEB CLOSE.
    WebSessionToken,
    /// Fullword input capacity for the HTTP method result.
    WebMethodLength,
    /// Fullword input capacity for the HTTP version result.
    WebVersionLength,
    /// Fullword input capacity for the HTTP authentication realm result.
    WebRealmLength,
    /// HTTP header name to read.
    WebHttpHeaderName,
    /// Query parameter name to read.
    WebQueryParmName,
    /// Form field name to read.
    WebFormFieldName,
    /// Fullword byte length of a Web read name.
    WebNameLength,
    /// Fullword receiving capacity for a Web read value.
    WebValueLength,
    /// Optional name at which a Web browse starts.
    WebBrowseStartName,
    /// HTTP header value to stage for WEB WRITE.
    WebHeaderValue,
    /// Outbound HTTP client method.
    WebMethod,
    /// IMMEDIATE or EVENTUAL server response action.
    WebAction,
    /// CLOSE or NOCLOSE message disposition.
    WebCloseStatus,
    /// Document token used as the response or request body.
    WebDocumentToken,
    /// HTTP server response status code.
    WebStatusCode,
    /// HTTP server response reason phrase.
    WebStatusText,
    /// Byte length of the reason phrase.
    WebStatusLength,
    /// Message body source buffer.
    WebFrom,
    /// Byte length of the message body source.
    WebFromLength,
    /// Explicit client request path.
    WebPathInput,
    /// Escaped client request query string.
    WebQueryInput,
    /// HTTP message media type.
    WebMediaType,
    /// Client URIMAP selected for this request.
    WebSendUriMap,
    /// Maximum body bytes requested by WEB RECEIVE.
    WebReceiveMaxLength,
    /// Client status-text receiving capacity for WEB RECEIVE.
    WebReceiveStatusLength,
}
