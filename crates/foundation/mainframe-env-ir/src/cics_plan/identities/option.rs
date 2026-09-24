/// Flag option accepted by the typed CICS pilot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsPlanOption {
    /// Return immediately when a child is not finished.
    BtsNoSuspend,
    /// Select the activity acquired by this unit of work.
    BtsAcqActivity,
    /// Select the process acquired by this unit of work.
    BtsAcqProcess,
    ConversationNoQueue,
    ConversationNotruncate,
    ConversationDefresp,
    ConversationFmh,
    ConversationDataNotruncate,
    ConversationDataBuffer,
    ConversationDataLlid,
    ConversationDataInvite,
    ConversationDataLast,
    ConversationDataConfirm,
    ConversationDataWait,
    ConversationDataFmh,
    ConversationDataDefresp,
    /// BasicAuth token syntax.
    SecurityBasicAuth,
    /// JSON Web Token syntax.
    SecurityJwt,
    /// Registered opaque Kerberos token syntax.
    SecurityKerberos,
    /// Raw token bytes.
    SecurityBit,
    /// Base64-encoded token bytes.
    SecurityBase64,
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
    /// Return a 40-byte uppercase hexadecimal SHA-1 digest.
    DigestHex,
    /// Return a 20-byte binary SHA-1 digest.
    DigestBinary,
    /// Return a 28-byte base64 SHA-1 digest.
    DigestBase64,
    /// Retain an operator message for immediate action (descriptor 2).
    OperatorImmediate,
    /// Retain an operator message for eventual action (descriptor 3).
    OperatorEventual,
    /// Retain an operator message for critical eventual action (descriptor 11).
    OperatorCritical,
    /// Select subject fields from the client certificate.
    CertificateOwner,
    /// Select issuer fields from the client certificate.
    CertificateIssuer,
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
    TraceOn,
    TraceOff,
    TraceSystem,
    TraceUser,
    TraceEi,
    TraceSingle,
    TraceAccount,
    TraceMonitor,
    TracePerform,
    /// Browse HTTP request or response headers.
    WebBrowseHttpHeader,
    /// Browse URL query parameters.
    WebBrowseQueryParm,
    /// Browse HTML form fields.
    WebBrowseFormField,
    /// Retain the unread HTTP body after a short WEB RECEIVE.
    WebNotruncate,
    /// Return client response bytes without code-page conversion.
    WebNoClientConvert,
    /// Return inbound request bytes without code-page conversion.
    WebNoServerConvert,
}
