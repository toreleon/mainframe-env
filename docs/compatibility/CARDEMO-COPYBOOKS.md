# CardDemo reached compatibility copybooks

Status: **Owned bounded compatibility contract for CD-003**

mainframe-env does not copy IBM product source. The compiler generates nine
owned source-level definitions from the catalog in
`mainframe-env-compiler::compatibility`. The definitions cover only symbols and
layouts reached by the pinned CardDemo corpus and are not a general CICS, Db2,
or MQ SDK.

## Behavioral authorities

- `DFHAID` represents the reached 3270 AID bytes for Enter, Clear, PA1/PA2, and
  PF1–PF24. IBM documents DFHAID as the standard attention-identifier list and
  recommends hexadecimal definitions where character-valued copies are not
  portable: <https://www.ibm.com/support/pages/node/377047>.
- `DFHBMSCA` represents the reached standard field attributes and extended
  colors. IBM documents the meanings of `DFHBMUNP`, `DFHBMPRO`, `DFHBMFSE`,
  `DFHBMPRF`, `DFHBMASF`, and `DFHBMASB` in the BMS constants reference:
  <https://www.ibm.com/docs/en/cics-ts/6.x?topic=reference-bms-constants>.
- `SQLCA` is the 136-byte communication area with `SQLCODE`, the 70-byte
  `SQLERRM` payload, six `SQLERRD` integers, eleven warning bytes, and
  five-byte `SQLSTATE`, matching IBM's published COBOL structure:
  <https://www.ibm.com/docs/en/db2/12.1.x?topic=structures-sqlca>.
- `CMQGMOV`, `CMQMDV`, `CMQODV`, `CMQPMOV`, `CMQTML`, and `CMQV` represent the
  reached MQ control fields, options, identifiers, and reason values. IBM's
  MQGMO reference defines options and wait behavior:
  <https://www.ibm.com/docs/en/ibm-mq/9.3.x?topic=mqi-mqgmo-get-message-options>.
  IBM's MQTM reference fixes the reached trigger-message offsets and total
  length of 684 bytes:
  <https://www.ibm.com/docs/en/ibm-mq/9.3.x?topic=i-mqtm-trigger-message>.

## Boundary and verification

Every definition is repository-owned UTF-8 source with a named
behavioral disposition. Application libraries precede the compatibility
library explicitly. A duplicate member within one library, missing member,
unassigned file, invalid path, or changed library order fails before artifact
publication. The conformance corpus remains external and no runtime archive or
native object participates in compilation.

Verify with:

```text
cargo test -p mainframe-env-source -p mainframe-env-compiler
CARDEMO_CORPUS_DIR=<local-clean-pin> cargo xtask carddemo-closure --check
```
