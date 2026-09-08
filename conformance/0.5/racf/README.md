# RACF/SAF 0.5 conformance inputs

The command, RACROUTE, and supplied-class catalogs in this directory project
into the shared Conformance IR. They do not define a second RACF authority.

## Reading the publication

`command-language.json` is reviewed row content and is never written by a tool.
What a tool produces is the evidence beside it: `../tools/fetch_racf_topics.py`
selects the 60 topics the RACF Command Language Reference publishes the command
syntax in, and `../tools/extract_racf_html_syntax.py` projects what they say
into `../generated/racf-html-syntax-projection.json`.

Both read documentation topics, never the PDF. The reference draws no railroad
diagrams: a command's syntax is a table under a `<h2 class="sectiontitle">Syntax</h2>`,
and its operands are a `<dl class="parml">` under `Parameters`, where an operand
is a `dt` at the outermost `dl` depth and everything it accepts or contains is a
`dt` of a `dl` nested inside its `dd`. Reading the typeset page instead meant
recovering that nesting from indentation, which reached 25 of the 34 families
and promoted a segment's members to top level whenever the segment opened
inline.

The rule for the syntax table is position, not an attribute, and the difference
matters to anyone reproducing this by hand. The tool takes **every table in the
Syntax section**, after dropping the section's two boilerplate paragraphs by
their `data-hd-otherprops="nohelp"` attribute. 55 of the 60 selected topics do
mark that table `role="presentation"`, but four — DELUSER, RACDCERT EXPORT,
RACDCERT REKEY and RACPRMCK — give it `summary="Syntax of the ... command"` and
no role at all, and the RACDCERT umbrella topic `radcertg.htm` has no table in
its Syntax section, because that section says to read the function subtopics. A
reviewer who writes a `role="presentation"` selector gets nothing for those five
and has no way to tell that from the publication being silent.

The nesting is read to whatever depth the topic uses — six levels in ALTUSER and
ALTGROUP, five in ADDGROUP, ADDUSER, RALTER, SET and SETROPTS — and each term is
classified against its own parent. A child name the parent term restates, either
between keywords (`AT | ONLYAT`) or inside its own argument
(`MSGRECVR(NO | YES)`), is a value; any other child name is a member. "Member"
therefore covers both a segment's operands and an enumeration nothing in the
book restates in the term — `AUTH` over MASTER, ALL, INFO, CONS, IO and SYS — so
a member reported below the operand level is a question for the reviewer, not a
claim that its parent is a segment.

**The restatement is read across topics, not down one page.** The reference
writes the same operand two ways: ADDUSER hangs GENERAL, GLOBAL and SPECIFIC
under a bare `CTL` where ALTUSER writes `CTL (GENERAL | GLOBAL | SPECIFIC)` over
the same three terms, and RALTER writes bare `ACEE` and `MIXED` over YES and NO
where RDEFINE writes `ACEE( YES | NO )` and `MIXED( YES | NO )`. Read a page at a
time those come out as values in one topic and members in the other for markup
that says the same thing, which is a fact about typesetting. So the tool reads
all 60 topics for restatements before classifying any of them, and a term's
alternatives are the ones any topic states for that operand name. 52 operand
names are stated that way; the union changes the reading of 12 child names under
five of them, and every one is listed in the projection's
`cross_topic_restatements` with the term it was read from — CTL, DOM and
LOGCMDRESP in ADDUSER, ACEE and MIXED in RALTER. It carries a restatement and
nothing else: `LEVEL(message-level)` states no alternatives, so LEVEL's NB, ALL,
CE and IN stay members wherever they are read.

Because a name can still be a value under one term and a member under another,
`source_values` and `source_members` overlap. That overlap is reported as two
lists rather than one total, because the total said something it did not mean.
384 of the 403 names in it are the list restating a name one level below where
it introduces it — `OMVS` over `MEMLIMIT | NOMEMLIMIT` over
`MEMLIMIT(nonshared-memory-size)` makes MEMLIMIT a member of OMVS and a value of
the term between them, and both readings are true — so those are
`restated_one_level_down` and are about the `dl`, not the reference. The 19 in
`value_and_member_of_different_terms` are the ones worth a reviewer's attention:
ADDUSER's ALL is a member of AUTH and LEVEL and a value of
`ROUTCODE(ALL | NONE | routing-codes)`, and SET's ALL is a value of
`ALL | NONE | TYPE` and a member of DATABASE. The split is by occurrence, so a
name restated under one segment and written bare under another — ADDGROUP's GID,
under OMVS and OVM — is in the second list. The two partition the overlap, so
their sum is still recoverable and is not printed.

Selection is by href and the predicate is written out in the fetcher's module
docstring, because a reviewer has to be able to arrive at the same 60 topics
without running anything. 34 command families is an immutable denominator, so a
tree that does not publish exactly 34 stops the tool rather than producing a
short manifest.

To reproduce, writing publication bytes outside the repository:

```text
python3 conformance/0.5/tools/fetch_racf_topics.py \
  --destination "$TMPDIR/racf/topics" \
  --manifest conformance/0.5/generated/racf-topic-manifest.json
python3 conformance/0.5/tools/extract_racf_html_syntax.py \
  --topics "$TMPDIR/racf/topics" \
  --manifest conformance/0.5/generated/racf-topic-manifest.json \
  --catalog conformance/0.5/racf/command-language.json \
  --output conformance/0.5/generated/racf-html-syntax-projection.json
python3 -m unittest discover -s conformance/0.5/tools/tests \
  -t conformance/0.5/tools/tests
```

The manifest records each topic's digest against
`conformance/0.2/manifests/racf-saf-topics.json`, so the projection states which
pinned bytes it rests on rather than asking a reviewer to take it on trust.

The projection is a review input. It carries `coverage_credit: 0`, it is not a
normative catalog, and its `catalog_only` and `syntax_only` lists are questions
for review — a name the catalog carries that the Parameters tree does not reach,
and a keyword the syntax table draws that the Parameters tree does not define.
Neither is a defect claim, and neither may be applied to `command-language.json`
without a reviewed disposition per name.

`operand-dispositions.json` is the executable review record for all three
populations. Its catalog-only entries preserve the pinned topic identity and
record a named reviewer, the evidence basis, and the concrete emulator behavior
for each retained spelling. Its per-command publication dispositions classify every
`source_only` and `syntax_only` name as implemented, deliberately unimplemented,
context-only, or a catalog gap. `cargo xtask racf-catalog --check` requires exact
set equality with the projection, rejects any unapplied catalog gap, and
generates separate top-level operand and syntax-token inventories. Source-only
and deliberately unsupported retained catalog-only operands return
`UnsupportedCapability(racf-command-operand)` at command level. A deliberately
unsupported syntax-only token returns that problem only at its recorded command
or enclosing-operand context; it never shadows a positional or an unrelated
operand value. Context-only tokens have no unambiguous executable position and
remain analysis evidence. The review remains zero-credit and does not substitute
for licensed differential evidence.

The 0.5 development gate also runs a bounded, table-driven, pure-state
reference simulation over the same 34 command and 14 RACROUTE row identities.
It is intentionally independent of the production provider and has explicit
unknowns for installation exits, cryptographic material, undocumented database
internals, and RRSF transport timing. Its property, metamorphic, and mutant
tests provide development assurance only: they do not create a campaign file,
an oracle receipt, a differential binding, or licensed credit.

Licensed differential is enabled only when an approved adapter installs
`licensed-oracle.json` at this path. The file must validate against
`../schemas/racf-oracle-campaign.schema.json`, cover exactly the frozen 34
command and 14 RACROUTE rows, identify IBM z/OS 3.2 RACF/SAF, assert that the
environment is licensed, carry the approved adapter and licensed-environment
identity prefixes, declare external licensed execution and a fresh
release-certify campaign, and contain no credential, key, token, certificate,
or MFA material. Simulated, modeled, documentation-derived, historical, and
current-product origins fail before registry projection. Absence of the file
means `differential=pending`.

Each case binds the exact generated differential fixture digest and carries a
normalized JSON observation produced by the licensed adapter:

- command: `surface`, `keyword`, `status`, and redacted `records`;
- RACROUTE: `surface`, `keyword`, `status`, `states`, and redacted `result`.

The product driver executes the same selected route on the current candidate
and requires JSON equality with the adapter observation. Only then does it emit
the campaign file digest through the shared runner's oracle-receipt field. The
shared cache identity already binds candidate, catalog, spec, fixture, oracle,
environment, shard, row, gate, and obligation identities.

The campaign is an imported receipt, not a hand-authored expectation. After an
approved adapter supplies it, run:

```text
cargo xtask racf-catalog
cargo xtask spec --check
cargo xtask conformance --subsystem racf-saf --gate differential
```

Do not use official documentation, local product output, or a synthetic fixture
as a substitute for the licensed IBM execution receipt. Under the user-approved
2026-09-01 scoped completion policy, its absence keeps 0.5 at
`differential=0/48 pending`; the real campaign remains mandatory at the 0.17
`release-certify` hard gate.
