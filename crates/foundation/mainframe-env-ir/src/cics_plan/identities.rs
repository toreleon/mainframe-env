use super::CicsAssignOutput;

/// CICS operation selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOperation {
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
    /// Yield the issuing task once for redispatch.
    Suspend,
    /// Wait for one timer-event control area to be posted.
    WaitEvent,
    /// Wait for standard MVS posting of one ECB in a bounded external list.
    WaitExternal,
    /// Return one bounded set of task, terminal, and invocation context values.
    Assign,
    /// Discard the current full-BMS logical message, if one is being built.
    PurgeMessage,
    /// Schedule one local interval-control START data record.
    Start,
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
    /// Optional SPOOLWRITE transfer length.
    SpoolFlength,
}

/// Flag option accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOption {
    Accum,
    Formfeed,
    DefaultScreen,
    AlternateScreen,
    EraseAup,
    Print,
    Alarm,
    Frset,
    Paging,
    Last,
    Honeom,
    L40,
    L64,
    L80,
    ReleasePage,
    RetainPage,
    Autopage,
    CurrentPage,
    AllPages,
    NoAutopage,
    OperPurge,
    DefResp,
    NoWait,
    Rrn,
    Console,
    PrintMedium,
    Card,
    WpMedia1,
    WpMedia2,
    WpMedia3,
    Nleom,
    WpMedia4,
    /// Retain lowercase bytes on a subsequent 8775 partition receive.
    AsIs,
    /// The composite predicate requires all child events.
    EventAnd,
    /// The composite predicate requires any child event.
    EventOr,
    TimerAfter,
    TimerAt,
    TimerOn,
    AcqActivity,
    AcqProcess,
    /// Ignore and clear active abnormal-termination exits.
    Cancel,
    /// Suppress transaction-dump creation.
    NoDump,
    /// Reactivate the most recently canceled abnormal-termination exit.
    Reset,
    /// Establish a read-for-update context.
    Update,
    /// Roll back rather than commit at syncpoint.
    Rollback,
    /// Suppress default condition handling.
    NoHandle,
    /// Keep an enqueue until task termination.
    Task,
    /// Keep an enqueue until the current unit of work ends.
    Uow,
    /// Return `ENQBUSY` rather than suspending for a contended resource.
    NoSuspend,
    /// Erase the terminal buffer before mapped output is displayed.
    Erase,
    /// Use symbolic map cursor positioning.
    Cursor,
    /// Use the default date separator.
    DateSep,
    /// Use the default time separator.
    TimeSep,
    /// Unlock the terminal keyboard after output.
    FreeKb,
    /// Start a file browse at the first key greater than or equal to RIDFLD.
    Gteq,
    /// Match a keyed READ by the KEYLENGTH prefix of RIDFLD.
    Generic,
    /// Mark START data as containing function management headers.
    Fmh,
    /// Defer START work admission until a successful syncpoint.
    Protect,
    /// Wait for an expired START record rather than returning ENDDATA immediately.
    Wait,
    After,
    At,
    For,
    Until,
    /// Suppress the generated START request identifier in EIBREQID.
    NoCheck,
    /// Send only the initialized default data defined by a BMS map.
    MapOnly,
    /// Send only application data and its supplied BMS field attributes.
    DataOnly,
    /// Require a keyed READ to match the complete or generic RIDFLD key.
    Equal,
    /// Receive mapped input from the terminal that originated the transaction.
    Terminal,
    /// Allow deadlock timeout or ordinary purge to abend this wait.
    Purgeable,
    /// Ignore deadlock timeout or ordinary purge while this wait is active.
    NotPurgeable,
    /// Read the next temporary-storage item after the queue-wide cursor.
    Next,
    /// Replace an existing temporary-storage item instead of appending.
    RewriteTemporary,
    /// Select auxiliary storage when creating a temporary-storage queue.
    Auxiliary,
    /// Select main storage when creating a temporary-storage queue.
    Main,
    /// Require the named application major and minor version exactly.
    ExactMatch,
    /// Select the highest minor version at or above the named minimum.
    Minimum,
    /// Retain a LOAD ownership after the issuing task terminates.
    Hold,
    /// Preserve percent escapes and plus signs in document symbol lists.
    Unescaped,
    /// Omit bookmark and conversion tags from a document retrieval.
    DocumentDataOnly,
    /// Request CICS-key 64-bit storage.
    CicsDataKey64,
    /// Request user-key 64-bit storage.
    UserDataKey64,
    /// Retain 64-bit storage beyond task end.
    Shared64,
    /// Request an executable DSA for a below-bar location.
    Executable64,
    /// Fail immediately while the selected counter pool is rebuilding.
    CounterNoSuspend,
    /// Reserve the remaining numbers when a GET increment reaches the limit.
    CounterReduce,
    /// Rewind at limit or when the reservation exceeds the remaining range.
    CounterWrap,
    /// Retain a closed spool report.
    SpoolKeep,
    /// Delete a closed spool report.
    SpoolDelete,
    /// Emit output without carriage-control bytes.
    SpoolNoCc,
    /// Use ASA carriage control.
    SpoolAsa,
    /// Use machine carriage control.
    SpoolMcc,
    /// Create a print report.
    SpoolPrint,
    /// Create a punch report.
    SpoolPunch,
    /// Write a line-mode spool record.
    SpoolLine,
    /// Write a page-mode spool record.
    SpoolPage,
    TraceException,
    DumpComplete,
    DumpTask,
    DumpStorage,
    DumpProgram,
    DumpTerminal,
    DumpTables,
    DumpFct,
    DumpPct,
    DumpPpt,
    DumpSit,
    DumpTct,
    DumpTrt,
    DumpDct,
}

/// Named result binding written after the host result arrives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOutputName {
    /// Name of the 8775 partition that supplied terminal input.
    Partn,
    /// Status returned by CHECK TIMER.
    TimerStatus,
    /// Event name returned by RETRIEVE REATTACH EVENT.
    EventName,
    /// Child name returned by RETRIEVE SUBEVENT.
    SubEventName,
    /// Event type returned by an event retrieval.
    EventType,
    /// Status returned by TEST EVENT.
    FireStatus,
    /// Current named-counter value returned by GET or QUERY.
    CounterValue,
    /// Defined named-counter lower bound returned by QUERY.
    CounterMinimum,
    /// Defined named-counter upper bound returned by QUERY.
    CounterMaximum,
    /// Record payload destination.
    Into,
    /// Pointer receiving interpreter-owned retrieved storage.
    SetPointer,
    /// Eight-byte opaque AMODE(64) virtual-address result.
    SetPointer64,
    /// Returned browse record identifier.
    Ridfld,
    /// `TOKEN(...)` fullword update identifier returned by READ UPDATE.
    Token,
    /// Returned communication-area destination.
    Commarea,
    /// Primary response code destination.
    Resp,
    /// Secondary response code destination.
    Resp2,
    /// `ABSTIME(...)` packed-decimal destination.
    Abstime,
    /// `MILLISECONDS(...)` fullword-binary destination.
    Milliseconds,
    /// `MMDDYY(...)` character destination.
    Mmddyy,
    /// `MMDDYYYY(...)` character destination.
    Mmddyyyy,
    /// `TIME(...)` character destination.
    Time,
    /// `YYDDD(...)` character destination.
    Yyddd,
    /// `YYMMDD(...)` character destination.
    Yymmdd,
    /// `YYYYMMDD(...)` character destination.
    Yyyymmdd,
    /// One source-reviewed `ASSIGN` output destination.
    Assign(CicsAssignOutput),
    /// Actual record length destination for `READ`.
    Length,
    /// Retrieved `RTRANSID(...)` metadata destination.
    ReturnTransId,
    /// Retrieved `RTERMID(...)` metadata destination.
    ReturnTermId,
    /// Retrieved `QUEUE(...)` metadata destination.
    Queue,
    /// Current temporary-storage queue item count.
    NumItems,
    /// Generated 16-byte document token destination.
    DocumentToken,
    /// Current maximum retrieval size destination.
    DocumentSize,
    /// Web-service-control result WebAction.
    WebAction,
    /// Web-service-control result WebMessageId.
    WebMessageId,
    /// Web-service-control result WebRelatesUri.
    WebRelatesUri,
    /// Web-service-control result WebRelatesType.
    WebRelatesType,
    /// Web-service-control result WebEprInto.
    WebEprInto,
    /// Web-service-control result WebEprSet.
    WebEprSet,
    /// Web-service-control result WebEprLength.
    WebEprLength,
    /// XML element local name.
    ElementName,
    /// XML element local-name length.
    ElementNameLength,
    /// XML element namespace.
    ElementNamespace,
    /// XML element namespace length.
    ElementNamespaceLength,
    /// XML type local name.
    TypeName,
    /// XML type local-name length.
    TypeNameLength,
    /// XML type namespace.
    TypeNamespace,
    /// XML type namespace length.
    TypeNamespaceLength,
    /// `REQID(...)` fullword token returned by asynchronous journal output.
    JournalReqId,
    /// Eight-byte token returned by SPOOLOPEN.
    SpoolToken,
    /// Actual length of the SPOOLREAD record.
    SpoolToFlength,
    /// Generated identifier of a retained local transaction dump.
    DumpId,
}
