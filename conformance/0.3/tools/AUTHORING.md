# Authoring conventions for the COBOL language catalog

These conventions bind anyone editing the `forms` of a row in
`conformance/0.3/cobol/language.json`. They live beside the grammar tools
because the tools in this directory produce the numbers the authoring decisions
are taken against, and because the first convention below is not a matter of
taste: getting it wrong breaks statements in rows nobody was editing.

## What the comparison counts

`extract_cobol_html_grammar.py` projects the reference's own DITA diagrams and
marks each one `format` or `fragment`. A format is a way of writing the
statement. A fragment is a named phrase the statement may take — `when-phrase
Format`, `converting-phrase-1 Format 2` — which the reference draws separately
because the statement diagram references it, not because the statement has
another shape.

Only formats are comparable against a catalog form. `compare_cobol_grammar.py`
therefore counts `source_format_count` against `catalog_form_count`, and
`format_titles` lists formats only. Fragment keywords are still compared,
because a keyword the publication only reaches through a phrase fragment is
still a keyword of the statement.

Against the pinned 6.5 topics the reference publishes 83 statement formats and
12 phrase fragments across the 44 rows, and 15 rows publish more formats than
the row records. `JSON PARSE` is one format against seven diagrams, not seven
formats; `SET` is genuinely eight.

## Figurative constants are written as `fig-con-1`, never as the words

**This is binding, and it is global.** `cobol_form_keywords`
(`xtask/src/main.rs:3273`) harvests every all-uppercase word out of a row's
`forms` into that row's `grammar_keywords`, and `is_grammar_keyword`
(`crates/kernel/mainframe-env-compiler/src/hir/statement_grammar.rs:2547`)
unions the `grammar_keywords` of all 44 rows into one set. `is_operand_atom`
(`:2535`) rejects any token in that union, and `consume_operand` (`:2480`) is
the only way an operand is ever recognised. So a word written into one row's
form stops being a legal operand in every row.

The union is 144 words today. Transcribing the publication's keyword boxes
verbatim would add 113 more, and eleven of those are figurative constants:
`SPACE`, `SPACES`, `ZERO`, `ZEROES`, `ZEROS`, `LOW-VALUE`, `LOW-VALUES`,
`HIGH-VALUE`, `HIGH-VALUES`, `NULL`, `NULLS`. Authoring `WHEN SPACES OR
LOW-VALUES` into the `JSON GENERATE` row in one batch would make `MOVE SPACES
TO A` malformed in a batch nobody had touched.

That is measured, not argued. Adding `"SPACES"` and `"NULL"` to a single row's
`grammar_keywords` in the generated Rust and analysing three programs:

    MOVE SPACES TO A.                                        -> MECOB0102 "operand is malformed"
    SET P TO NULL.                                           -> MECOB0102 "operand is malformed"
    JSON GENERATE J FROM A CONVERTING A TO JSON NULL USING SPACES.  -> accepted

All three are accepted with the union as it stands. The JSON statement survives
because `json_conversion_value` (`:2706`) eats those nine words itself before it
ever reaches `atom`; `MOVE` and `SET` reach `consume_operand` and have no such
escape. The markup path would survive the change and the data-movement path
would not, which is precisely why this cannot be settled per batch.

**The rule.** Where a form needs a figurative constant, write the
publication's own operand name for it and not the words:

    CONVERTING identifier-7 TO JSON NULL USING fig-con-1

`fig-con-1` is the reference's own name, not an invention of ours. `JSON
GENERATE` and `JSON PARSE` both box it as a `syntaxvar` in their
`converting-phrase Format 2` diagrams, and the prose beneath defines it as
"one of the figurative constants from the list below: SPACE, SPACES ZERO,
ZEROES, ZEROS LOW-VALUE, LOW-VALUES HIGH-VALUE, HIGH-VALUES" — the same nine
words `json_conversion_value` special-cases. Being lowercase, `fig-con-1` is
harvested as a placeholder rather than a keyword, so it reserves nothing.

**`NULL` and `NULLS` in `SET` are the exception that proves the rule, and they
are not exempt.** The publication does box them as keywords, in `SET` Formats 5,
6 and 7, and offers no `fig-con` name for them there. (An earlier statement of
this ruling cited those three formats as already publishing `fig-con-1`; they do
not — they publish `NULL` and `NULLS`, and `fig-con-1` appears only in the two
JSON converting-phrase diagrams. The ruling is unchanged; the citation was
wrong.) Writing them anyway breaks `SET P TO NULL`, as the probe above shows —
it breaks the very format being authored. So they stay out of the form, and the
row records the omission in `disposition` rather than leaving it silent.
Admitting them would need a `SET`-side special case in `statement_grammar.rs`
alongside the one `json_conversion_value` already has, which is a Rust change
and not an authoring one.

## `disposition` records a deliberate under-description

`$defs/statement-family` in `conformance/spec/schemas/cobol-language.schema.json`
takes an optional `disposition`:

    "disposition": {
      "rationale": "Formats 5 to 7 differ only in the pointer flavour of the receiver; the NULL and NULLS branches are omitted because reserving those words breaks SET itself.",
      "source_formats": 8
    }

Absent means the forms are the whole story. Present means the row publishes
fewer forms than the reference publishes formats *on purpose*, and says why.
`rationale` is required and has a 16-character floor so it cannot be filled in
with "n/a"; `source_formats` optionally states the count the row is being
measured against, which is `source_format_count` for that row in
`conformance/0.3/generated/cobol-grammar-comparison.json`.

The field is inert with respect to the generated Rust: `render_cobol_language`
iterates a fixed list of field names, so adding it changes nothing under
`crates/kernel/mainframe-env-compiler/src/generated/`.
