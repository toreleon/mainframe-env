# MQ inquiry attribute source boundary

Status: **Normative source boundary; zero execution credit**
Owner: MQ source-review maintainers
Scope: four retained inquiry attribute sources and remaining semantic gaps
Applies from: mainframe-env mq.programming

`mq-inquiry-attribute-sources` is a separately registered, external-only MQ 9.4
source-review scope under `ibm-mq-9.4-inquiry-attribute-sources-2026-09-12`.
Its four-topic [manifest](../../../../conformance/subsystems/mq/manifests/mq-inquiry-attribute-sources-topics.json)
uses `mainframe-env.topic-manifest@1`; the shared registry fixes semantic authority
to false and coverage credit to zero. This note is a source reference, not a
numeric runtime selector catalog, executable permission or acceptance receipt.

## Exact source identities

All topic paths start with `SSFKSJ_9.4.0/refdev/` and end with `.html`.

| Topic | Attribute or constants | SHA-256 | Bytes | Publication last modified |
|---|---|---|---:|---|
| q092480_ | MQQT queue types | ac5de9d74f62635456e566bfcbba7706699686cef14580ab30aa22166b2fb1ba | 4661 | 2026-05-18 |
| q102690_ | QMgrName | d488b1433fe1fe6b5051459af841e02801df4d62d43bbfc3f9f0954575b411b4 | 1247 | 2026-05-18 |
| q103420_ | QName | 27228d1ae3bf316c77949becc2ed9b26fe9bb4aad97f974a4d31b95caccc3861 | 2105 | 2026-05-18 |
| q103490_ | QType | 93bd55632f95b85f9790e3cba75c623bff93c1a679983115546edf32ce527639 | 2267 | 2026-05-18 |

The manifest file SHA-256 is
`2a015ae7e74819b603d2b07a0d7af5eef172623550be86f7fc8e74189b3eb294`;
the sorted topic-set digest is
`463e5ba10a4b572cd5a73ff08066820b66c59e3133b37910ecd4c1c9af5ce527`.
The unchanged MQ 9.4 TOC digest is
`5b23147424db490f5292bd56afe1a0dd2a6ccdde3a08388a79d599998e002bd4`.
Its topicIds are `constants-mqqt-queue-types-extended-queue-types`,
`manager-qmgrname-mqchar48`, `queues-qname-mqchar48` and `queues-qtype-mqlong`.
Official endpoints follow the manifest's versioned content URL template.

The selected archive run is `SSFKSJ_9.4.0-9c2e93a27e4d8eff/20260912T015856Z-39397b4d`.
Exact metadata fetch times are 2026-09-12T04:10:49Z (MQQT), 04:17:35Z
(QMgrName), 04:19:20Z (QName) and 04:19:26Z (QType). Publication dates above
come from each hash-verified HTML `lastModifiedDate`, not these fetch times.
The archive remains in-progress and predates the MQINQ issue337 re-pin. No
independent browser reproduction, freshness, complete corpus or same-snapshot
claim follows from heading, endpoint, metadata or retained-byte agreement.
Publication bodies remain outside Git.

## Reviewed joins and applicability

Parser locators below use the shared offline reader's one-based plain-text lines.
The original call remains MQINQ, official row
`ibm-mq-9.4-mqi-2026-08-31:mqi-calls-unique:0016`, source position16,
q101840_ SHA
`03e3347bbf16d2f8e3a9061e921dbfca7a3afd0fe3bc13418ebdf47bb652ce1b`.
The original call manifest, supplements80 and all existing projections remain exact.

Selector numbers belong to the already pinned supplemental selector tables:

| Selector | Decimal | Hexadecimal | Existing topic / lines |
|---|---:|---|---|
| MQCA_Q_NAME | 2016 | 0x000007E0 | q090430_, 190–192 |
| MQCA_Q_MGR_NAME | 2015 | 0x000007DF | q090430_, 187–189 |
| MQIA_Q_TYPE | 20 | 0x00000014 | q091590_, 451–453 |

Those topics use `ibm-mq-9.4-programming-supplements-2026-09-12`; this note does
not introduce another selector authority or convert the host's pending selectors.
Their SHAs are respectively
`674a9d56b087f39fd257d6935d25254745a185525eeec9c7aeb6552892b29146` and
`33468f1a42a59c46286c019ac8d8023970f65f90685cb7836dc77ffc3d630e2e`.

q092480_ lines11–25 give LOCAL=1, MODEL=2, ALIAS=3, REMOTE=6 and CLUSTER=7;
each separate hexadecimal cell agrees. Lines34–36 declare the extended ALL=1001.
ALL is not a returned concrete QType. q103490_ lines19–28 list only alias,
cluster, local and remote as QType results. Its applicability table, and QName's,
has marked Local/Alias/Remote/Cluster columns and an unmarked Model column.
The original HTML column structure must be retained when reviewing these tables;
the plain-text separators alone do not identify an empty column reliably.
q101840_ lines817–818 explain that opening a model creates a dynamic local queue
and inquiry observes that dynamic queue. The MODEL constant does not authorize a
model QType result on that handle.

QName is declared MQCHAR48 (48 character positions), names a queue defined on
the connected local manager, and uses MQCA_Q_NAME/MQ_Q_NAME_LENGTH (q103420_
lines1–21). Queue definitions share one namespace; local and alias definitions
cannot share a name. QMgrName is also MQCHAR48, identifies the connected local
manager, and uses MQCA_Q_MGR_NAME/MQ_Q_MGR_NAME_LENGTH (q102690_ lines1–6).
On z/OS its nonblank subsystem name is limited to four characters; this does not
shorten the fixed returned field to four bytes. These field declarations do not
independently define a CCSID or the numeric length-constant table. No new character
conversion or empty/unset policy is admitted by registration.

## Call output and remaining closure

q101840_ lines14/21 require an actual connected HCONN and an HOBJ opened with
MQOO_INQUIRE. The CICS-specific default connection exception in lines15–18 is
not an ordinary batch exemption. Source selectors and a valid binding are not
real SAF, original-core, current-frame, current-unit or physical-store permission.

SelectorCount accepts 0..256 (line24). Selectors can interleave integer and
character occurrences; output retains relative order within each family
(lines41–48). A future adapter must preserve repeated occurrences, never sort or
deduplicate them. Integer capacity counts slots; character capacity counts bytes
(lines706–725). Sufficient capacity returns the requested values; excess output
storage is unchanged. Character values have their fixed attribute extent and are
right blank-padded. Zero count/length makes the corresponding output unreferenced.
These rules do not turn a missing optional catalog value into an empty name.

Invalid object selectors fail; queue selectors valid only for another queue type
produce a warning (lines30–40). The source specifies a non-applicable integer
symbol and all-asterisk character fields (lines714/724), but this four-topic scope
does not pin the integer sentinel's numeric mapping. Warning reasons are subtype
2068, integer shortage2022 and character shortage2008; precedence is that order
(lines740–746,853–856). The reviewed paragraphs do not establish a complete
short-buffer partial-copy algorithm or all-selectors validation timing relative
to partial output. Do not guess truncation or normalize a warning to success.

Inquiry sees a snapshot (line816). Alias handles inquire the alias object's
attributes, not resolved base queue/topic attributes (lines819–821). Cluster
QType and available attributes depend on actual open/resolution mode
(lines822–847); registry presence is not cluster applicability. Future native
closure still needs explicit output encoding/CCSID, numeric length/sentinel joins,
empty/unset semantics, short-output definedness, validation ordering and each
admitted object/open profile. No additional topic is implicitly accepted here.

Derived source checks must reject changed hashes/counts/duplicates/scope/version
or positive credit; distinguish ALIAS=3 from REMOTE=6; preserve the empty Model
table cell; and reject MODEL/ALL as inferred concrete QType results. They must
not equate integer slots with bytes, substitute base names for alias names,
collapse duplicate selectors, map unset to blank, or invent output on failure.
These are source-bound regression obligations, not executed MQINQ runtime tests.

## Offline reproduction and credit

Import only the four approved HTML files and pinned TOC using the existing shared
reader contract in the [cache runbook](../../../runbooks/IBM-DOCS-CACHE.md).
Then run `ibm_docs.py --cache EXTERNAL_CACHE status`, `search` and `read` with
`--scope mq-inquiry-attribute-sources`; select exact SHA for each read. Check the
configured retained topic path first, then matching archive bytes. Never refresh
or repin implicitly. The shared checker validates manifest/registry closure without
mounting publication bodies; no new reader or schema is introduced.

Source presence and reference review earn zero execution/native/installed/official
or licensed credit. Numeric MQINQ forwarding and MQSET remain pending; no public
profile is promoted. The ten pending reason declarations and official26/27 remain
unchanged. Licensed oracle alone is human-skipped0/26; all other full mq.programming
security, durability, recovery, participant, IR and CardDemo acceptance remains
required.
