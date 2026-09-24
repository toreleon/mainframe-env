use super::super::{CicsAssignOutput, CicsCertificateOutput, CicsTcpipOutput};

/// Named result binding written after the host result arrives.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CicsOutputName {
    /// Source-visible APPC ISSUE state CVDA.
    IssueState,
    /// Twenty-four-byte GDS conversation indicator area.
    IssueConvData,
    /// Six-byte GDS return code.
    IssueRetCode,
    /// Opaque token selected by FETCH ANY.
    BtsAny,
    /// Child completion CVDA.
    BtsCompStatus,
    /// Reply channel name fetched from a child.
    BtsChannel,
    /// Four-character child abend code.
    BtsAbcode,
    AttachProcess,
    AttachResource,
    AttachReturnProcess,
    AttachReturnResource,
    AttachQueue,
    AttachIuType,
    AttachDataStream,
    AttachRecordFormat,
    ConversationState,
    ConversationData,
    ConversationRetCode,
    LogonInto,
    LogonSet,
    LogonLength,
    ProcessName,
    ProcessLength,
    SyncLevel,
    PipList,
    PipLength,
    TctSysId,
    TctTermId,
    ConversationConvid,
    ConversationRetcode,
    ConversationPrinConvid,
    ConversationPrinSysid,
    ConversationConvData,
    ConversationInto,
    ConversationSet,
    ConversationToLength,
    ConversationToFullLength,
    /// Verified eight-character user identity.
    SecurityIsUserId,
    /// Four-byte token encryption-key handle.
    SecurityEncryptKey,
    /// Mutual-authentication token pointer and length.
    SecurityOutToken,
    SecurityOutTokenLength,
    /// Encrypted PassTicket pointer and ciphertext length.
    SecurityEncryptPassTicket,
    SecurityEncryptLength,
    /// Eight-character PassTicket output from SAF.
    SecurityPassTicket,
    /// Three-character terminal language selected by SIGNON.
    SecurityLangInUse,
    /// One-character terminal language selected by SIGNON.
    SecurityNatLangInUse,
    /// QUERY SECURITY READ access CVDA.
    SecurityRead,
    /// QUERY SECURITY UPDATE access CVDA.
    SecurityUpdate,
    /// QUERY SECURITY CONTROL access CVDA.
    SecurityControl,
    /// QUERY SECURITY ALTER access CVDA.
    SecurityAlter,
    /// Credential change time returned by SAF.
    SecurityChangeTime,
    /// Remaining credential lifetime in days.
    SecurityDaysLeft,
    /// External security manager reason code.
    SecurityEsmReason,
    /// External security manager response code.
    SecurityEsmResp,
    /// Credential expiry time returned by SAF.
    SecurityExpiryTime,
    /// Invalid credential attempt count.
    SecurityInvalidCount,
    /// Prior successful-use time.
    SecurityLastUseTime,
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
    /// In-place `FIELD(...)` result of BIF DEEDIT.
    Field,
    /// `RESULT(...)` destination of BIF DIGEST.
    DigestResult,
    /// Operator reply bytes received through `REPLY(...)`.
    OperatorReply,
    /// Actual operator reply byte count.
    OperatorReplyLength,
    /// One source-reviewed EXTRACT CERTIFICATE result.
    Certificate(CicsCertificateOutput),
    /// One source-reviewed EXTRACT TCPIP result.
    Tcpip(CicsTcpipOutput),
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
    /// Uppercase scheme returned by WEB PARSE URL.
    WebSchemeName,
    /// Host name or unbracketed IP literal.
    WebHost,
    /// Actual host length, including on truncation.
    WebHostLength,
    /// HOSTNAME, IPV4, or IPV6 result.
    WebHostType,
    /// Explicit or scheme-default port number.
    WebPortNumber,
    /// URL path component.
    WebPath,
    /// Actual path length, including on truncation.
    WebPathLength,
    /// Escaped query string component.
    WebQueryString,
    /// Actual query string length, including on truncation.
    WebQueryStringLength,
    /// Generated eight-byte client session token.
    WebSessionToken,
    /// Server HTTP major protocol number.
    WebHttpVNum,
    /// Server HTTP minor protocol number.
    WebHttpRNum,
    /// HTTP or HTTPS scheme CVDA.
    WebScheme,
    /// HTTP request method.
    WebHttpMethod,
    /// Actual HTTP method length.
    WebMethodLength,
    /// HTTP protocol version string.
    WebHttpVersion,
    /// Actual HTTP version string length.
    WebVersionLength,
    /// HTTPYES or HTTPNO request-type CVDA.
    WebRequestType,
    /// Selected inbound or client URIMAP name.
    WebUriMap,
    /// Latest HTTP 401 authentication realm.
    WebRealm,
    /// Actual realm length.
    WebRealmLength,
    /// Value returned by WEB READ.
    WebValue,
    /// Actual WEB READ value length.
    WebValueLength,
    /// Name returned by WEB READNEXT.
    WebBrowseName,
    /// Actual WEB READNEXT name length.
    WebBrowseNameLength,
    /// Document token returned by WEB RETRIEVE.
    WebRetrieveDocumentToken,
    /// Body bytes returned by WEB RECEIVE.
    WebReceiveInto,
    /// Actual WEB RECEIVE body byte count.
    WebReceiveLength,
    /// Client response status code returned by WEB RECEIVE.
    WebReceiveStatusCode,
    /// Client response reason returned by WEB RECEIVE.
    WebReceiveStatusText,
    /// Actual client response reason length.
    WebReceiveStatusLength,
    /// HTTP content media type returned by WEB RECEIVE.
    WebReceiveMediaType,
    /// HTTP body charset returned by WEB RECEIVE.
    WebReceiveBodyCharset,
    /// Response body returned by WEB CONVERSE.
    WebConverseInto,
    /// Actual WEB CONVERSE body length.
    WebConverseToLength,
    /// Client response status code returned by WEB CONVERSE.
    WebConverseStatusCode,
    /// Client response reason returned by WEB CONVERSE.
    WebConverseStatusText,
    /// Actual client response reason length.
    WebConverseStatusLength,
    /// Client response media type returned by WEB CONVERSE.
    WebConverseMediaType,
    /// Client response body charset returned by WEB CONVERSE.
    WebConverseBodyCharset,
}
