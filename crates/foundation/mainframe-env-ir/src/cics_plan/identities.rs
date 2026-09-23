use super::CicsAssignOutput;

/// CICS operation selected by the frontend.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOperation {
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
    /// Allocate one bounded task-local virtual storage area.
    Getmain,
    /// Change the issuing task's dispatch priority and optionally yield.
    ChangeTask,
    /// Cancel one unhonored local interval-control START request.
    Cancel,
    /// Complete a source-defined zero-delay request without suspension.
    Delay,
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
    /// Read one item from a local temporary-storage queue.
    ReadTemporaryStorage,
    /// Append or replace one item in a local temporary-storage queue.
    WriteTemporaryStorage,
    /// Receive one mapped terminal input message.
    ReceiveMap,
    /// Send one mapped terminal output message.
    SendMap,
    /// Send one unmapped terminal text message.
    SendText,
    /// Rewrite the record held by the current update context.
    Rewrite,
    /// Commit or roll back the current unit of work.
    Syncpoint,
    /// Overwrite the originating task's bounded user correlator data.
    SetAssociationUserCorrData,
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
}

/// Named input accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOperandName {
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
    /// `INITIMG(...)` one-byte initialization image.
    InitImage,
    /// `DATAPOINTER(...)` virtual storage pointer returned by GETMAIN.
    DataPointer,
    /// `DATA(...)` area whose virtual address identifies GETMAIN storage.
    DataArea,
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
    /// Element-name buffer length.
    ElementNameLength,
    /// Element-namespace buffer length.
    ElementNamespaceLength,
    /// Type-name buffer length.
    TypeNameLength,
    /// Type-namespace buffer length.
    TypeNamespaceLength,
}

/// Flag option accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOption {
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
}

/// Named result binding written after the host result arrives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOutputName {
    /// Record payload destination.
    Into,
    /// Pointer receiving interpreter-owned retrieved storage.
    SetPointer,
    /// Returned browse record identifier.
    Ridfld,
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
}
