use super::{
    ControlEdgeKind, ControlScope, HirProblem, StatementKind, StatementOption, StatementOptionKind,
    statement_options,
};
use crate::PROCEDURE_STATEMENTS;
use std::collections::BTreeSet;
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct ProcedureSyntax {
    pub events: Vec<ProcedureEvent>,
    pub event_sentences: Vec<usize>,
    pub sentence_count: usize,
}

#[derive(Clone, Debug)]
pub(super) enum ProcedureEvent {
    Statement {
        kind: StatementKind,
        header: Range<usize>,
        range: Range<usize>,
        line: usize,
        options: Vec<StatementOption>,
        scope: Option<ControlScope>,
    },
    Branch {
        range: Range<usize>,
        line: usize,
        expected: Vec<ControlScope>,
        edge: ControlEdgeKind,
    },
    ConditionBranch {
        range: Range<usize>,
        line: usize,
        owner: usize,
    },
    ScopeEnd {
        scope: ControlScope,
        range: Range<usize>,
        line: usize,
    },
    Terminator {
        range: Range<usize>,
        line: usize,
    },
    Label {
        name: String,
        section: bool,
        range: Range<usize>,
        line: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Word,
    Number,
    Literal,
    Punctuation,
}

#[derive(Clone, Debug)]
struct Token<'a> {
    text: &'a str,
    range: Range<usize>,
    line: usize,
    kind: TokenKind,
}

impl Token<'_> {
    fn is(&self, value: &str) -> bool {
        self.text.eq_ignore_ascii_case(value)
    }
}

#[derive(Clone, Copy)]
enum SequenceStop {
    If,
    Evaluate,
    Search,
    Perform,
    Options(StatementKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerformMode {
    Inline,
    OutOfLine,
}

#[derive(Clone, Copy)]
struct BranchMatch {
    length: usize,
    option: StatementOptionKind,
    rank: u8,
    group: u8,
}

pub(super) fn parse(source: &str, max_statements: usize) -> Result<ProcedureSyntax, HirProblem> {
    GrammarParser::new(source, max_statements)?.parse()
}

struct GrammarParser<'a> {
    source: &'a str,
    tokens: Vec<Token<'a>>,
    position: usize,
    sentence: usize,
    statement_count: usize,
    max_statements: usize,
    events: Vec<ProcedureEvent>,
    event_sentences: Vec<usize>,
    sequence_owners: Vec<StatementKind>,
}

impl<'a> GrammarParser<'a> {
    fn new(source: &'a str, max_statements: usize) -> Result<Self, HirProblem> {
        Ok(Self {
            source,
            tokens: lex(source)?,
            position: 0,
            sentence: 0,
            statement_count: 0,
            max_statements,
            events: Vec::new(),
            event_sentences: Vec::new(),
            sequence_owners: Vec::new(),
        })
    }

    fn parse(mut self) -> Result<ProcedureSyntax, HirProblem> {
        while self.position < self.tokens.len() {
            if self.at_period() {
                self.position += 1;
                self.sentence += 1;
                continue;
            }
            if self.is_label() {
                self.parse_label()?;
            } else {
                self.parse_statement()?;
            }
        }
        Ok(ProcedureSyntax {
            events: self.events,
            event_sentences: self.event_sentences,
            sentence_count: self.sentence.saturating_add(1),
        })
    }

    fn parse_label(&mut self) -> Result<(), HirProblem> {
        self.reserve_statement()?;
        let start = self.position;
        let line = self.tokens[start].line;
        let name = self.tokens[start].text.to_ascii_uppercase();
        self.position += 1;
        let section = self.at_word("SECTION");
        if section {
            self.position += 1;
        }
        if !self.at_period() {
            return Err(HirProblem::UnknownStatement(line));
        }
        self.push_event(ProcedureEvent::Label {
            name,
            section,
            range: self.token_range(start, self.position),
            line,
        });
        Ok(())
    }

    fn parse_statement(&mut self) -> Result<(), HirProblem> {
        if self.at_any_terminator() || self.at_word("ELSE") || self.at_word("WHEN") {
            return Err(HirProblem::UnmatchedScope);
        }
        let (kind, keyword_length) = self
            .classify_at(self.position)
            .ok_or_else(|| HirProblem::UnknownStatement(self.current_line()))?;
        self.reserve_statement()?;
        match kind {
            StatementKind::If => self.parse_if(),
            StatementKind::Evaluate => self.parse_evaluate(),
            StatementKind::Search => self.parse_search(),
            StatementKind::Perform => self.parse_perform(),
            StatementKind::ExecCics | StatementKind::ExecDli | StatementKind::ExecSql => {
                self.parse_exec(kind, keyword_length)
            }
            _ => self.parse_simple(kind, keyword_length),
        }
    }

    fn parse_simple(
        &mut self,
        kind: StatementKind,
        keyword_length: usize,
    ) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let header_end = self.find_simple_header_end(kind, start, keyword_length)?;
        let header = self.token_range(start, header_end);
        let event = self.push_statement_event(kind, header.clone(), None);
        self.position = header_end;
        let mut options = statement_options(kind, &self.source[header.clone()]);
        let mut seen = BTreeSet::new();
        let mut last_rank = 0u8;
        let mut group = None;
        while let Some(branch) = self.branch_at(kind, self.position) {
            if !seen.insert(branch.option) || branch.rank < last_rank {
                return Err(self.invalid(kind, line, "duplicate or reordered conditional phrase"));
            }
            if let Some(previous) = group
                && previous != branch.group
                && matches!(kind, StatementKind::Read)
            {
                return Err(self.invalid(kind, line, "mutually exclusive READ conditions"));
            }
            group = Some(branch.group);
            last_rank = branch.rank;
            let phrase_start = self.position;
            self.position += branch.length;
            let phrase_range = self.token_range(phrase_start, self.position);
            let owner = event;
            self.push_event(ProcedureEvent::ConditionBranch {
                range: phrase_range,
                line: self.tokens[phrase_start].line,
                owner,
            });
            options.push(StatementOption {
                kind: branch.option,
                operands: Vec::new(),
            });
            let before = self.statement_count;
            self.parse_sequence(SequenceStop::Options(kind), Some(kind))?;
            if self.statement_count == before {
                return Err(self.invalid(kind, line, "conditional phrase has no statement body"));
            }
        }
        let had_branches = !seen.is_empty();
        if self.at_terminator(kind) {
            let terminator = self.position;
            self.position += 1;
            self.push_event(ProcedureEvent::Terminator {
                range: self.token_range(terminator, self.position),
                line: self.tokens[terminator].line,
            });
            options.push(StatementOption {
                kind: StatementOptionKind::ExplicitTerminator,
                operands: Vec::new(),
            });
        } else if had_branches {
            let range = self
                .tokens
                .get(self.position)
                .filter(|token| token.kind == TokenKind::Punctuation && token.text == ".")
                .map_or_else(
                    || {
                        let end = self.previous_end();
                        end..end
                    },
                    |token| token.range.clone(),
                );
            self.push_event(ProcedureEvent::Terminator { range, line });
        }
        options.sort_by_key(|option| option.kind);
        options.dedup_by_key(|option| option.kind);
        let end = self.previous_end().max(header.end);
        self.finish_statement_event(event, end, options);
        Ok(())
    }

    fn parse_if(&mut self) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let body = self.find_nested_body_start(StatementKind::If, start + 1, |tokens| {
            let condition = if tokens.last().is_some_and(|token| token.is("THEN")) {
                &tokens[..tokens.len() - 1]
            } else {
                tokens
            };
            validate_expression(condition)
        })?;
        let condition_end = body
            - usize::from(
                self.tokens
                    .get(body.saturating_sub(1))
                    .is_some_and(|token| token.is("THEN")),
            );
        let header = self.token_range(start, condition_end);
        let event = self.push_statement_event(StatementKind::If, header, Some(ControlScope::If));
        self.position = body;
        let before = self.statement_count;
        self.parse_sequence(SequenceStop::If, Some(StatementKind::If))?;
        if self.statement_count == before {
            return Err(self.invalid(StatementKind::If, line, "IF has no imperative statement"));
        }
        if self.at_word("ELSE") {
            let branch = self.position;
            self.position += 1;
            self.push_event(ProcedureEvent::Branch {
                range: self.token_range(branch, self.position),
                line: self.tokens[branch].line,
                expected: vec![ControlScope::If],
                edge: ControlEdgeKind::False,
            });
            let before = self.statement_count;
            self.parse_sequence(SequenceStop::If, Some(StatementKind::If))?;
            if self.statement_count == before {
                return Err(self.invalid(StatementKind::If, line, "ELSE has no statement body"));
            }
        }
        let mut options = Vec::new();
        let end = self.close_structured_scope(
            StatementKind::If,
            ControlScope::If,
            "END-IF",
            &mut options,
        )?;
        self.finish_statement_event(event, end, options);
        Ok(())
    }

    fn parse_evaluate(&mut self) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let when = self
            .find_word_at_depth(start + 1, "WHEN")
            .ok_or_else(|| self.invalid(StatementKind::Evaluate, line, "EVALUATE requires WHEN"))?;
        if !validate_expression(&self.tokens[start + 1..when]) {
            return Err(self.invalid(StatementKind::Evaluate, line, "invalid EVALUATE subjects"));
        }
        let event = self.push_statement_event(
            StatementKind::Evaluate,
            self.token_range(start, when),
            Some(ControlScope::Evaluate),
        );
        self.position = when;
        let mut branches = 0usize;
        let mut saw_other = false;
        while self.at_word("WHEN") {
            if saw_other {
                return Err(self.invalid(
                    StatementKind::Evaluate,
                    line,
                    "WHEN OTHER must be the last branch",
                ));
            }
            let branch_start = self.position;
            self.position += 1;
            let (body, has_body) =
                self.find_branch_body_boundary(StatementKind::Evaluate, self.position)?;
            saw_other = self.position + 1 == body && self.tokens[self.position].is("OTHER");
            self.push_event(ProcedureEvent::Branch {
                range: self.token_range(branch_start, body),
                line: self.tokens[branch_start].line,
                expected: vec![ControlScope::Evaluate],
                edge: ControlEdgeKind::Branch,
            });
            self.position = body;
            if has_body {
                let before = self.statement_count;
                self.parse_sequence(SequenceStop::Evaluate, Some(StatementKind::Evaluate))?;
                if self.statement_count == before {
                    return Err(self.invalid(
                        StatementKind::Evaluate,
                        line,
                        "WHEN has no imperative statement",
                    ));
                }
            } else if !self.at_word("WHEN") {
                return Err(self.invalid(StatementKind::Evaluate, line, "WHEN body is missing"));
            }
            branches += 1;
        }
        if branches == 0 {
            return Err(self.invalid(StatementKind::Evaluate, line, "EVALUATE has no WHEN branch"));
        }
        let mut options = Vec::new();
        let end = self.close_structured_scope(
            StatementKind::Evaluate,
            ControlScope::Evaluate,
            "END-EVALUATE",
            &mut options,
        )?;
        self.finish_statement_event(event, end, options);
        Ok(())
    }

    fn parse_search(&mut self) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let mut boundary = None;
        let mut index = start + 1;
        let mut depth = 0usize;
        while index < self.tokens.len() {
            if depth == 0
                && (self.tokens[index].is("WHEN")
                    || self.phrase_at(index, &["AT", "END"])
                    || self.tokens[index].is("END-SEARCH")
                    || self.is_period_at(index))
            {
                boundary = Some(index);
                break;
            }
            adjust_depth(&self.tokens[index], &mut depth)?;
            index += 1;
        }
        let boundary = boundary.unwrap_or(self.tokens.len());
        validate_search_header(&self.tokens[start..boundary])
            .map_err(|detail| self.invalid(StatementKind::Search, line, detail))?;
        let event = self.push_statement_event(
            StatementKind::Search,
            self.token_range(start, boundary),
            Some(ControlScope::Search),
        );
        self.position = boundary;
        if self.phrase_at(self.position, &["AT", "END"]) {
            let branch = self.position;
            self.position += 2;
            self.push_event(ProcedureEvent::Branch {
                range: self.token_range(branch, self.position),
                line: self.tokens[branch].line,
                expected: vec![ControlScope::Search],
                edge: ControlEdgeKind::Branch,
            });
            let before = self.statement_count;
            self.parse_sequence(SequenceStop::Search, Some(StatementKind::Search))?;
            if self.statement_count == before {
                return Err(self.invalid(
                    StatementKind::Search,
                    line,
                    "AT END has no statement body",
                ));
            }
        }
        let mut branches = 0usize;
        while self.at_word("WHEN") {
            let branch = self.position;
            self.position += 1;
            let body = self.find_nested_body_start(
                StatementKind::Search,
                self.position,
                validate_expression,
            )?;
            self.push_event(ProcedureEvent::Branch {
                range: self.token_range(branch, body),
                line: self.tokens[branch].line,
                expected: vec![ControlScope::Search],
                edge: ControlEdgeKind::Branch,
            });
            self.position = body;
            let before = self.statement_count;
            self.parse_sequence(SequenceStop::Search, Some(StatementKind::Search))?;
            if self.statement_count == before {
                return Err(self.invalid(
                    StatementKind::Search,
                    line,
                    "WHEN has no statement body",
                ));
            }
            branches += 1;
        }
        if branches == 0 {
            return Err(self.invalid(StatementKind::Search, line, "SEARCH has no WHEN branch"));
        }
        let mut options = Vec::new();
        let end = self.close_structured_scope(
            StatementKind::Search,
            ControlScope::Search,
            "END-SEARCH",
            &mut options,
        )?;
        self.finish_statement_event(event, end, options);
        Ok(())
    }

    fn parse_perform(&mut self) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let mut index = start + 1;
        let mut depth = 0usize;
        while index < self.tokens.len() {
            if depth == 0 {
                if self.tokens[index].is("END-PERFORM") || self.is_period_at(index) {
                    break;
                }
                if self.classify_at(index).is_some() {
                    let header = &self.tokens[start..index];
                    let mode = validate_perform_header(header)
                        .map_err(|detail| self.invalid(StatementKind::Perform, line, detail))?;
                    return match mode {
                        PerformMode::OutOfLine => {
                            self.push_complete_simple(StatementKind::Perform, start, index, None)
                        }
                        PerformMode::Inline => self.parse_inline_perform(start, index),
                    };
                }
            }
            adjust_depth(&self.tokens[index], &mut depth)?;
            index += 1;
        }
        let mode = validate_perform_header(&self.tokens[start..index])
            .map_err(|detail| self.invalid(StatementKind::Perform, line, detail))?;
        match mode {
            PerformMode::OutOfLine => {
                self.push_complete_simple(StatementKind::Perform, start, index, None)
            }
            PerformMode::Inline => Err(self.invalid(
                StatementKind::Perform,
                line,
                "inline PERFORM has no statement body",
            )),
        }
    }

    fn parse_inline_perform(&mut self, start: usize, body: usize) -> Result<(), HirProblem> {
        let line = self.tokens[start].line;
        let event = self.push_statement_event(
            StatementKind::Perform,
            self.token_range(start, body),
            Some(ControlScope::Perform),
        );
        self.position = body;
        let before = self.statement_count;
        self.parse_sequence(SequenceStop::Perform, Some(StatementKind::Perform))?;
        if self.statement_count == before {
            return Err(self.invalid(StatementKind::Perform, line, "inline PERFORM body is empty"));
        }
        let mut options = Vec::new();
        let end = self.close_structured_scope(
            StatementKind::Perform,
            ControlScope::Perform,
            "END-PERFORM",
            &mut options,
        )?;
        self.finish_statement_event(event, end, options);
        Ok(())
    }

    fn parse_exec(&mut self, kind: StatementKind, keyword_length: usize) -> Result<(), HirProblem> {
        let start = self.position;
        let line = self.tokens[start].line;
        let end = (start + keyword_length..self.tokens.len())
            .find(|index| self.tokens[*index].is("END-EXEC"))
            .ok_or(HirProblem::UnterminatedExec)?;
        if end == start + keyword_length {
            return Err(self.invalid(kind, line, "EXEC statement body is empty"));
        }
        self.position = end + 1;
        self.push_complete_simple(kind, start, self.position, None)
    }

    fn parse_sequence(
        &mut self,
        stop: SequenceStop,
        owner: Option<StatementKind>,
    ) -> Result<(), HirProblem> {
        if let Some(owner) = owner {
            self.sequence_owners.push(owner);
        }
        let result = (|| {
            while self.position < self.tokens.len()
                && !self.at_period()
                && !self.at_sequence_stop(stop)
            {
                if self.at_any_terminator() || self.at_word("ELSE") || self.at_word("WHEN") {
                    return Err(owner.map_or(HirProblem::UnmatchedScope, |kind| {
                        self.invalid(kind, self.current_line(), "unexpected branch or terminator")
                    }));
                }
                if self.classify_at(self.position).is_none() {
                    return Err(owner.map_or_else(
                        || HirProblem::UnknownStatement(self.current_line()),
                        |kind| {
                            self.invalid(
                                kind,
                                self.current_line(),
                                "unknown token in nested statement body",
                            )
                        },
                    ));
                }
                if let Err(problem) = self.parse_statement() {
                    return Err(match (owner, problem) {
                        (Some(kind), HirProblem::InvalidStatement { .. }) => self.invalid(
                            kind,
                            self.current_line(),
                            "malformed nested statement body",
                        ),
                        (_, problem) => problem,
                    });
                }
            }
            Ok(())
        })();
        if owner.is_some() {
            self.sequence_owners.pop();
        }
        result
    }

    fn find_simple_header_end(
        &self,
        kind: StatementKind,
        start: usize,
        keyword_length: usize,
    ) -> Result<usize, HirProblem> {
        let line = self.tokens[start].line;
        let mut index = start + keyword_length;
        let mut depth = 0usize;
        while index < self.tokens.len() {
            let exit_qualifier = kind == StatementKind::Exit
                && index == start + 1
                && [
                    "PROGRAM",
                    "METHOD",
                    "FUNCTION",
                    "PERFORM",
                    "PARAGRAPH",
                    "SECTION",
                ]
                .iter()
                .any(|word| self.tokens[index].is(word));
            if depth == 0
                && (self.at_simple_stop(kind, index)
                    || (!exit_qualifier && self.classify_at(index).is_some()))
            {
                return validate_simple_header(kind, &self.tokens[start..index])
                    .map(|()| index)
                    .map_err(|detail| self.invalid(kind, line, detail));
            }
            if depth == 0
                && self.sequence_owners.iter().rev().any(|owner| {
                    self.branch_at(*owner, index).is_some() || self.at_terminator_at(*owner, index)
                })
            {
                return validate_simple_header(kind, &self.tokens[start..index])
                    .map(|()| index)
                    .map_err(|detail| self.invalid(kind, line, detail));
            }
            adjust_depth(&self.tokens[index], &mut depth)?;
            index += 1;
        }
        validate_simple_header(kind, &self.tokens[start..index])
            .map(|()| index)
            .map_err(|detail| self.invalid(kind, line, detail))
    }

    fn find_nested_body_start(
        &self,
        kind: StatementKind,
        start: usize,
        validator: impl Fn(&[Token<'_>]) -> bool,
    ) -> Result<usize, HirProblem> {
        let line = self.tokens[start.saturating_sub(1)].line;
        let mut index = start;
        let mut depth = 0usize;
        while index < self.tokens.len() {
            if depth == 0 {
                if self.is_period_at(index)
                    || self.tokens[index].is("ELSE")
                    || self.tokens[index].is("WHEN")
                    || self.at_any_terminator_at(index)
                {
                    break;
                }
                if self.classify_at(index).is_some() {
                    if validator(&self.tokens[start..index]) {
                        return Ok(index);
                    }
                    return Err(self.invalid(
                        kind,
                        line,
                        "invalid condition or nested-body boundary",
                    ));
                }
            }
            adjust_depth(&self.tokens[index], &mut depth)?;
            index += 1;
        }
        Err(self.invalid(kind, line, "missing nested imperative statement"))
    }

    fn find_branch_body_boundary(
        &self,
        kind: StatementKind,
        start: usize,
    ) -> Result<(usize, bool), HirProblem> {
        let line = self.tokens[start.saturating_sub(1)].line;
        let mut index = start;
        let mut depth = 0usize;
        while index < self.tokens.len() {
            if depth == 0 {
                if self.classify_at(index).is_some() {
                    if validate_expression(&self.tokens[start..index]) {
                        return Ok((index, true));
                    }
                    return Err(self.invalid(kind, line, "invalid branch objects"));
                }
                if self.tokens[index].is("WHEN")
                    || self.tokens[index].is("END-EVALUATE")
                    || self.is_period_at(index)
                {
                    if validate_expression(&self.tokens[start..index]) {
                        return Ok((index, false));
                    }
                    return Err(self.invalid(kind, line, "invalid branch objects"));
                }
            }
            adjust_depth(&self.tokens[index], &mut depth)?;
            index += 1;
        }
        Err(self.invalid(kind, line, "missing branch body"))
    }

    fn close_structured_scope(
        &mut self,
        kind: StatementKind,
        scope: ControlScope,
        terminator: &str,
        options: &mut Vec<StatementOption>,
    ) -> Result<usize, HirProblem> {
        let line = self.current_line();
        if self.at_word(terminator) {
            let end = self.position;
            self.position += 1;
            let range = self.token_range(end, self.position);
            let offset = range.end;
            self.push_event(ProcedureEvent::ScopeEnd {
                scope,
                range,
                line: self.tokens[end].line,
            });
            options.push(StatementOption {
                kind: StatementOptionKind::ExplicitTerminator,
                operands: Vec::new(),
            });
            Ok(offset)
        } else if self.at_period() {
            let range = self.tokens[self.position].range.clone();
            let offset = self.previous_end();
            self.push_event(ProcedureEvent::ScopeEnd {
                scope,
                range,
                line: self.tokens[self.position].line,
            });
            Ok(offset)
        } else {
            Err(self.invalid(kind, line, "missing or mismatched scope terminator"))
        }
    }

    fn push_complete_simple(
        &mut self,
        kind: StatementKind,
        start: usize,
        end: usize,
        scope: Option<ControlScope>,
    ) -> Result<(), HirProblem> {
        let range = self.token_range(start, end);
        let event = self.push_statement_event(kind, range.clone(), scope);
        self.position = end;
        self.finish_statement_event(
            event,
            range.end,
            statement_options(kind, &self.source[range]),
        );
        Ok(())
    }

    fn push_statement_event(
        &mut self,
        kind: StatementKind,
        header: Range<usize>,
        scope: Option<ControlScope>,
    ) -> usize {
        let event = self.events.len();
        self.push_event(ProcedureEvent::Statement {
            kind,
            range: header.clone(),
            header,
            line: self.tokens[self.position].line,
            options: Vec::new(),
            scope,
        });
        event
    }

    fn push_event(&mut self, event: ProcedureEvent) -> usize {
        let index = self.events.len();
        self.events.push(event);
        self.event_sentences.push(self.sentence);
        index
    }

    fn finish_statement_event(
        &mut self,
        event: usize,
        end: usize,
        mut options: Vec<StatementOption>,
    ) {
        options.sort_by_key(|option| option.kind);
        options.dedup_by_key(|option| option.kind);
        if let ProcedureEvent::Statement {
            range,
            options: stored,
            ..
        } = &mut self.events[event]
        {
            range.end = end;
            *stored = options;
        }
    }

    fn reserve_statement(&mut self) -> Result<(), HirProblem> {
        if self.statement_count >= self.max_statements {
            return Err(HirProblem::StatementLimitExceeded);
        }
        self.statement_count += 1;
        Ok(())
    }

    fn at_sequence_stop(&self, stop: SequenceStop) -> bool {
        match stop {
            SequenceStop::If => self.at_word("ELSE") || self.at_word("END-IF"),
            SequenceStop::Evaluate => self.at_word("WHEN") || self.at_word("END-EVALUATE"),
            SequenceStop::Search => self.at_word("WHEN") || self.at_word("END-SEARCH"),
            SequenceStop::Perform => self.at_word("END-PERFORM"),
            SequenceStop::Options(kind) => {
                self.branch_at(kind, self.position).is_some() || self.at_terminator(kind)
            }
        }
    }

    fn at_simple_stop(&self, kind: StatementKind, index: usize) -> bool {
        self.is_period_at(index)
            || self.branch_at(kind, index).is_some()
            || self.at_terminator_at(kind, index)
            || self.at_any_terminator_at(index)
            || self.tokens[index].is("ELSE")
            || self.tokens[index].is("WHEN")
    }

    fn is_label(&self) -> bool {
        if self.classify_at(self.position).is_some()
            || self.tokens[self.position].kind != TokenKind::Word
            || self.tokens[self.position]
                .text
                .to_ascii_uppercase()
                .starts_with("END-")
            || self.tokens[self.position].is("ELSE")
            || self.tokens[self.position].is("WHEN")
        {
            return false;
        }
        self.is_period_at(self.position + 1)
            || (self.token_is(self.position + 1, "SECTION") && self.is_period_at(self.position + 2))
    }

    fn classify_at(&self, index: usize) -> Option<(StatementKind, usize)> {
        classify(&self.tokens, index)
    }

    fn branch_at(&self, kind: StatementKind, index: usize) -> Option<BranchMatch> {
        branch_match(&self.tokens, kind, index)
    }

    fn at_terminator(&self, kind: StatementKind) -> bool {
        self.at_terminator_at(kind, self.position)
    }

    fn at_terminator_at(&self, kind: StatementKind, index: usize) -> bool {
        terminator(kind).is_some_and(|word| self.token_is(index, word))
    }

    fn at_any_terminator(&self) -> bool {
        self.at_any_terminator_at(self.position)
    }

    fn at_any_terminator_at(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Word && token.text.starts_with("END-"))
    }

    fn at_word(&self, word: &str) -> bool {
        self.token_is(self.position, word)
    }

    fn token_is(&self, index: usize, word: &str) -> bool {
        self.tokens.get(index).is_some_and(|token| token.is(word))
    }

    fn phrase_at(&self, index: usize, phrase: &[&str]) -> bool {
        phrase
            .iter()
            .enumerate()
            .all(|(offset, word)| self.token_is(index + offset, word))
    }

    fn at_period(&self) -> bool {
        self.is_period_at(self.position)
    }

    fn is_period_at(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Punctuation && token.text == ".")
    }

    fn find_word_at_depth(&self, start: usize, word: &str) -> Option<usize> {
        let mut depth = 0usize;
        for index in start..self.tokens.len() {
            if depth == 0 && self.tokens[index].is(word) {
                return Some(index);
            }
            if adjust_depth(&self.tokens[index], &mut depth).is_err() {
                return None;
            }
            if depth == 0 && self.is_period_at(index) {
                return None;
            }
        }
        None
    }

    fn token_range(&self, start: usize, end: usize) -> Range<usize> {
        let start_offset = self
            .tokens
            .get(start)
            .map_or(self.source.len(), |token| token.range.start);
        let end_offset = if end > start {
            self.tokens[end - 1].range.end
        } else {
            start_offset
        };
        start_offset..end_offset
    }

    fn previous_end(&self) -> usize {
        self.position
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map_or(0, |token| token.range.end)
    }

    fn current_line(&self) -> usize {
        self.tokens
            .get(self.position)
            .or_else(|| self.tokens.last())
            .map_or(1, |token| token.line)
    }

    fn invalid(&self, kind: StatementKind, line: usize, detail: &'static str) -> HirProblem {
        HirProblem::InvalidStatement { kind, line, detail }
    }
}

fn lex(source: &str) -> Result<Vec<Token<'_>>, HirProblem> {
    let mut tokens = Vec::new();
    let mut offset = 0usize;
    let mut line = 1usize;
    while offset < source.len() {
        let rest = &source[offset..];
        if rest.starts_with("*>") {
            let length = rest.find('\n').unwrap_or(rest.len());
            offset += length;
            continue;
        }
        let first = rest
            .chars()
            .next()
            .ok_or(HirProblem::MalformedToken(line))?;
        if first.is_whitespace() {
            line += usize::from(first == '\n');
            offset += first.len_utf8();
            continue;
        }
        let start = offset;
        let (kind, length) = if matches!(first, '\'' | '"') {
            (
                TokenKind::Literal,
                quoted_length(rest, 0).ok_or(HirProblem::MalformedToken(line))?,
            )
        } else if first.is_ascii_alphabetic() {
            let word_length = rest
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                .count();
            if rest
                .as_bytes()
                .get(word_length)
                .is_some_and(|byte| matches!(byte, b'\'' | b'"'))
                && matches!(
                    rest[..word_length].to_ascii_uppercase().as_str(),
                    "B" | "BX" | "G" | "N" | "NX" | "U" | "X" | "Z"
                )
            {
                (
                    TokenKind::Literal,
                    quoted_length(rest, word_length).ok_or(HirProblem::MalformedToken(line))?,
                )
            } else {
                (TokenKind::Word, word_length)
            }
        } else if first.is_alphabetic() {
            (
                TokenKind::Word,
                rest.char_indices()
                    .take_while(|(_, character)| {
                        character.is_alphanumeric() || matches!(character, '-' | '_')
                    })
                    .last()
                    .map_or(first.len_utf8(), |(index, character)| {
                        index + character.len_utf8()
                    }),
            )
        } else if first.is_ascii_digit() {
            let integer = rest.bytes().take_while(u8::is_ascii_digit).count();
            let fraction = if rest.as_bytes().get(integer) == Some(&b'.')
                && rest
                    .as_bytes()
                    .get(integer + 1)
                    .is_some_and(u8::is_ascii_digit)
            {
                1 + rest.as_bytes()[integer + 1..]
                    .iter()
                    .take_while(|byte| byte.is_ascii_digit())
                    .count()
            } else {
                0
            };
            (TokenKind::Number, integer + fraction)
        } else {
            let two = rest.get(..2).unwrap_or(rest);
            let length = if matches!(two, ">=" | "<=" | "<>" | "**") {
                2
            } else {
                first.len_utf8()
            };
            (TokenKind::Punctuation, length)
        };
        if length == 0 || start + length > source.len() {
            return Err(HirProblem::MalformedToken(line));
        }
        offset += length;
        tokens.push(Token {
            text: &source[start..offset],
            range: start..offset,
            line,
            kind,
        });
    }
    Ok(tokens)
}

fn quoted_length(text: &str, prefix: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let quote = *bytes.get(prefix)?;
    let mut index = prefix + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
            } else {
                return Some(index + 1);
            }
        } else {
            index += text[index..].chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

fn classify(tokens: &[Token<'_>], index: usize) -> Option<(StatementKind, usize)> {
    let token = tokens.get(index)?;
    for (first, second, kind) in [
        ("STOP", "RUN", StatementKind::StopRun),
        ("GO", "BACK", StatementKind::GoBack),
        ("GO", "TO", StatementKind::GoTo),
        ("NEXT", "SENTENCE", StatementKind::NextSentence),
        ("JSON", "GENERATE", StatementKind::JsonGenerate),
        ("JSON", "PARSE", StatementKind::JsonParse),
        ("XML", "GENERATE", StatementKind::XmlGenerate),
        ("XML", "PARSE", StatementKind::XmlParse),
        ("EXEC", "CICS", StatementKind::ExecCics),
        ("EXEC", "DLI", StatementKind::ExecDli),
        ("EXEC", "SQL", StatementKind::ExecSql),
    ] {
        if token.is(first) && tokens.get(index + 1).is_some_and(|token| token.is(second)) {
            return Some((kind, 2));
        }
    }
    Some((
        match token.text.to_ascii_uppercase().as_str() {
            "ACCEPT" => StatementKind::Accept,
            "ADD" => StatementKind::Add,
            "ALLOCATE" => StatementKind::Allocate,
            "ALTER" => StatementKind::Alter,
            "CALL" => StatementKind::Call,
            "CANCEL" => StatementKind::Cancel,
            "CLOSE" => StatementKind::Close,
            "COMPUTE" => StatementKind::Compute,
            "CONTINUE" => StatementKind::Continue,
            "DELETE" => StatementKind::Delete,
            "DISPLAY" => StatementKind::Display,
            "DIVIDE" => StatementKind::Divide,
            "ENTRY" => StatementKind::Entry,
            "EVALUATE" => StatementKind::Evaluate,
            "EXIT" => StatementKind::Exit,
            "FREE" => StatementKind::Free,
            "GOBACK" => StatementKind::GoBack,
            "IF" => StatementKind::If,
            "INITIALIZE" => StatementKind::Initialize,
            "INSPECT" => StatementKind::Inspect,
            "INVOKE" => StatementKind::Invoke,
            "MERGE" => StatementKind::Merge,
            "MOVE" => StatementKind::Move,
            "MULTIPLY" => StatementKind::Multiply,
            "OPEN" => StatementKind::Open,
            "PERFORM" => StatementKind::Perform,
            "READ" => StatementKind::Read,
            "RELEASE" => StatementKind::Release,
            "RETURN" => StatementKind::ReturnStatement,
            "REWRITE" => StatementKind::Rewrite,
            "SEARCH" => StatementKind::Search,
            "SET" => StatementKind::Set,
            "SORT" => StatementKind::Sort,
            "START" => StatementKind::Start,
            "STRING" => StatementKind::String,
            "SUBTRACT" => StatementKind::Subtract,
            "UNSTRING" => StatementKind::Unstring,
            "WRITE" => StatementKind::Write,
            _ => return None,
        },
        1,
    ))
}

fn branch_match(tokens: &[Token<'_>], kind: StatementKind, index: usize) -> Option<BranchMatch> {
    let at = |phrase: &[&str]| phrase_at(tokens, index, phrase);
    let matched = |length, option, rank, group| BranchMatch {
        length,
        option,
        rank,
        group,
    };
    match kind {
        StatementKind::Accept
        | StatementKind::Call
        | StatementKind::Invoke
        | StatementKind::JsonGenerate
        | StatementKind::JsonParse
        | StatementKind::XmlGenerate
        | StatementKind::XmlParse => {
            if at(&["NOT", "ON", "EXCEPTION"]) {
                Some(matched(3, StatementOptionKind::NotOnException, 1, 0))
            } else if at(&["ON", "EXCEPTION"]) {
                Some(matched(2, StatementOptionKind::OnException, 0, 0))
            } else {
                None
            }
        }
        StatementKind::Add
        | StatementKind::Compute
        | StatementKind::Divide
        | StatementKind::Multiply
        | StatementKind::Subtract => {
            if at(&["NOT", "ON", "SIZE", "ERROR"]) {
                Some(matched(4, StatementOptionKind::NotOnSizeError, 1, 0))
            } else if at(&["ON", "SIZE", "ERROR"]) {
                Some(matched(3, StatementOptionKind::OnSizeError, 0, 0))
            } else {
                None
            }
        }
        StatementKind::Delete | StatementKind::Rewrite | StatementKind::Start => {
            if at(&["NOT", "INVALID", "KEY"]) {
                Some(matched(3, StatementOptionKind::NotInvalidKey, 1, 0))
            } else if at(&["INVALID", "KEY"]) {
                Some(matched(2, StatementOptionKind::InvalidKey, 0, 0))
            } else {
                None
            }
        }
        StatementKind::Read => {
            if at(&["NOT", "AT", "END"]) {
                Some(matched(3, StatementOptionKind::NotAtEnd, 1, 0))
            } else if at(&["AT", "END"]) {
                Some(matched(2, StatementOptionKind::AtEnd, 0, 0))
            } else if at(&["NOT", "INVALID", "KEY"]) {
                Some(matched(3, StatementOptionKind::NotInvalidKey, 1, 1))
            } else if at(&["INVALID", "KEY"]) {
                Some(matched(2, StatementOptionKind::InvalidKey, 0, 1))
            } else {
                None
            }
        }
        StatementKind::ReturnStatement => {
            if at(&["NOT", "AT", "END"]) {
                Some(matched(3, StatementOptionKind::NotAtEnd, 1, 0))
            } else if at(&["AT", "END"]) {
                Some(matched(2, StatementOptionKind::AtEnd, 0, 0))
            } else {
                None
            }
        }
        StatementKind::String | StatementKind::Unstring => {
            if at(&["NOT", "ON", "OVERFLOW"]) {
                Some(matched(3, StatementOptionKind::NotOnOverflow, 1, 0))
            } else if at(&["ON", "OVERFLOW"]) {
                Some(matched(2, StatementOptionKind::OnOverflow, 0, 0))
            } else {
                None
            }
        }
        StatementKind::Write => {
            if at(&["NOT", "AT", "END-OF-PAGE"]) {
                Some(matched(3, StatementOptionKind::NotAtEnd, 1, 0))
            } else if at(&["AT", "END-OF-PAGE"]) {
                Some(matched(2, StatementOptionKind::AtEnd, 0, 0))
            } else if at(&["NOT", "INVALID", "KEY"]) {
                Some(matched(3, StatementOptionKind::NotInvalidKey, 1, 1))
            } else if at(&["INVALID", "KEY"]) {
                Some(matched(2, StatementOptionKind::InvalidKey, 0, 1))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn terminator(kind: StatementKind) -> Option<&'static str> {
    Some(match kind {
        StatementKind::Accept => "END-ACCEPT",
        StatementKind::Add => "END-ADD",
        StatementKind::Call => "END-CALL",
        StatementKind::Compute => "END-COMPUTE",
        StatementKind::Delete => "END-DELETE",
        StatementKind::Divide => "END-DIVIDE",
        StatementKind::Invoke => "END-INVOKE",
        StatementKind::JsonGenerate | StatementKind::JsonParse => "END-JSON",
        StatementKind::Multiply => "END-MULTIPLY",
        StatementKind::Read => "END-READ",
        StatementKind::ReturnStatement => "END-RETURN",
        StatementKind::Rewrite => "END-REWRITE",
        StatementKind::Start => "END-START",
        StatementKind::String => "END-STRING",
        StatementKind::Subtract => "END-SUBTRACT",
        StatementKind::Unstring => "END-UNSTRING",
        StatementKind::Write => "END-WRITE",
        StatementKind::XmlGenerate | StatementKind::XmlParse => "END-XML",
        _ => return None,
    })
}

fn validate_simple_header(kind: StatementKind, tokens: &[Token<'_>]) -> Result<(), &'static str> {
    match kind {
        StatementKind::Accept => validate_accept(tokens),
        StatementKind::Add => validate_add_subtract(tokens, "TO", true),
        StatementKind::Allocate => validate_allocate(tokens),
        StatementKind::Alter => validate_alter(tokens),
        StatementKind::Call => validate_call_like(tokens, false),
        StatementKind::Cancel => validate_cancel(tokens),
        StatementKind::Close => validate_close(tokens),
        StatementKind::Compute => validate_compute(tokens),
        StatementKind::Continue | StatementKind::NextSentence => {
            if tokens.len() == keyword_length(kind) {
                Ok(())
            } else {
                Err("statement does not accept operands")
            }
        }
        StatementKind::GoBack => {
            if (tokens.len() == 1 && tokens[0].is("GOBACK"))
                || (tokens.len() == 2 && tokens[0].is("GO") && tokens[1].is("BACK"))
            {
                Ok(())
            } else {
                Err("statement does not accept operands")
            }
        }
        StatementKind::Delete => validate_delete(tokens),
        StatementKind::Display => validate_display(tokens),
        StatementKind::Divide => validate_divide(tokens),
        StatementKind::Entry => validate_entry(tokens),
        StatementKind::Exit => validate_exit(tokens),
        StatementKind::Free => validate_free(tokens),
        StatementKind::GoTo => validate_go_to(tokens),
        StatementKind::Initialize => validate_initialize(tokens),
        StatementKind::Inspect => validate_inspect(tokens),
        StatementKind::Invoke => validate_call_like(tokens, true),
        StatementKind::JsonGenerate | StatementKind::XmlGenerate => validate_generate(tokens),
        StatementKind::JsonParse => validate_json_parse(tokens),
        StatementKind::XmlParse => validate_xml_parse(tokens),
        StatementKind::Merge => validate_sort_merge(tokens, false),
        StatementKind::Move => validate_move(tokens),
        StatementKind::Multiply => validate_multiply(tokens),
        StatementKind::Open => validate_open(tokens),
        StatementKind::Read => validate_read(tokens),
        StatementKind::Release => validate_release(tokens),
        StatementKind::ReturnStatement => validate_return(tokens),
        StatementKind::Rewrite => validate_rewrite(tokens),
        StatementKind::Set => validate_set(tokens),
        StatementKind::Sort => validate_sort_merge(tokens, true),
        StatementKind::Start => validate_start(tokens),
        StatementKind::StopRun => validate_stop(tokens),
        StatementKind::String => validate_string(tokens),
        StatementKind::Subtract => validate_add_subtract(tokens, "FROM", false),
        StatementKind::Unstring => validate_unstring(tokens),
        StatementKind::Write => validate_write(tokens),
        StatementKind::Perform
        | StatementKind::If
        | StatementKind::Evaluate
        | StatementKind::Search
        | StatementKind::ExecCics
        | StatementKind::ExecDli
        | StatementKind::ExecSql
        | StatementKind::DuplicateLabel
        | StatementKind::StructuredControl
        | StatementKind::Label
        | StatementKind::ProgramEnd => Err("statement requires its dedicated grammar"),
    }
}

fn validate_accept(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if cursor.eat("FROM") {
        let source = cursor.position;
        cursor.operand()?;
        if tokens.get(source).is_some_and(|token| token.is("DATE")) {
            cursor.eat("YYYYMMDD");
        } else if tokens.get(source).is_some_and(|token| token.is("DAY")) {
            cursor.eat("YYYYDDD");
        } else if tokens
            .get(source)
            .is_some_and(|token| token.is("ENVIRONMENT"))
        {
            cursor.operand()?;
        }
    }
    cursor.finish()
}

fn validate_add_subtract(
    tokens: &[Token<'_>],
    separator: &str,
    allow_direct_giving: bool,
) -> Result<(), &'static str> {
    if tokens
        .get(1)
        .is_some_and(|token| token.is("CORRESPONDING") || token.is("CORR"))
    {
        if !allow_direct_giving {
            return Err("CORRESPONDING is not valid for this arithmetic statement");
        }
        let mut cursor = Cursor::new(tokens, 2);
        cursor.operand()?;
        cursor.expect(separator)?;
        cursor.operand()?;
        cursor.eat("ROUNDED");
        return cursor.finish();
    }
    let split = find_word(tokens, 1, separator)
        .or_else(|| {
            allow_direct_giving
                .then(|| find_word(tokens, 1, "GIVING"))
                .flatten()
        })
        .ok_or("required arithmetic separator is missing")?;
    validate_operand_list(&tokens[1..split], 1)?;
    if tokens[split].is("GIVING") {
        return validate_arithmetic_receivers(&tokens[split + 1..]);
    }
    let giving = find_word(tokens, split + 1, "GIVING");
    let receivers_end = giving.unwrap_or(tokens.len());
    validate_arithmetic_receivers(&tokens[split + 1..receivers_end])?;
    if let Some(giving) = giving {
        validate_arithmetic_receivers(&tokens[giving + 1..])?;
    }
    Ok(())
}

fn validate_arithmetic_receivers(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut position = 0usize;
    let mut count = 0usize;
    while position < tokens.len() {
        if tokens[position].text == "," {
            if count == 0 || position + 1 == tokens.len() {
                return Err("receiver separator is misplaced");
            }
            position += 1;
            continue;
        }
        position = consume_operand(tokens, position).ok_or("receiver is malformed")?;
        if tokens
            .get(position)
            .is_some_and(|token| token.is("ROUNDED"))
        {
            position += 1;
        }
        count += 1;
    }
    if count == 0 {
        Err("required receiver is missing")
    } else {
        Ok(())
    }
}

fn validate_allocate(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.eat("CHARACTERS");
    cursor.eat("INITIALIZED");
    if cursor.eat("RETURNING") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_alter(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.expect("TO")?;
    if cursor.eat("PROCEED") {
        cursor.expect("TO")?;
    }
    cursor.operand()?;
    cursor.finish()
}

fn validate_call_like(tokens: &[Token<'_>], invoke: bool) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if invoke {
        cursor.operand()?;
    }
    if cursor.eat("USING") {
        let mut count = 0usize;
        while !cursor.done() && !cursor.at("RETURNING") {
            if cursor.eat("BY")
                && !(cursor.eat("REFERENCE") || cursor.eat("CONTENT") || cursor.eat("VALUE"))
            {
                return Err("invalid CALL argument mode");
            }
            cursor.operand()?;
            count += 1;
        }
        if count == 0 {
            return Err("USING requires an argument");
        }
    }
    if cursor.eat("RETURNING") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_close(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    let mut files = 0usize;
    while !cursor.done() {
        cursor.operand()?;
        files += 1;
        let reel_or_unit = cursor.eat("REEL") || cursor.eat("UNIT");
        if cursor.eat("WITH") {
            if cursor.eat("LOCK") {
                if reel_or_unit {
                    return Err("WITH LOCK cannot follow REEL or UNIT");
                }
            } else {
                cursor.expect("NO")?;
                cursor.expect("REWIND")?;
            }
        } else if cursor.eat("FOR") {
            if !reel_or_unit {
                return Err("FOR REMOVAL requires REEL or UNIT");
            }
            cursor.expect("REMOVAL")?;
        }
        if cursor.at("REEL")
            || cursor.at("UNIT")
            || cursor.at("WITH")
            || cursor.at("FOR")
            || cursor.at("NO")
            || cursor.at("LOCK")
            || cursor.at("REMOVAL")
            || cursor.at("REWIND")
        {
            return Err("duplicate, conflicting, or misplaced CLOSE phrase");
        }
    }
    (files > 0).then_some(()).ok_or("CLOSE requires a file")
}

fn validate_cancel(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    let mut targets = 0usize;
    while !cursor.done() {
        let target = cursor
            .tokens
            .get(cursor.position)
            .ok_or("CANCEL target is missing")?;
        if !matches!(target.kind, TokenKind::Word | TokenKind::Literal)
            || is_grammar_keyword(target)
        {
            return Err("CANCEL target must be a program name or identifier");
        }
        cursor.operand()?;
        targets += 1;
    }
    (targets > 0)
        .then_some(())
        .ok_or("CANCEL requires at least one target")
}

fn validate_free(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    let mut pointers = 0usize;
    while !cursor.done() {
        let pointer = cursor
            .tokens
            .get(cursor.position)
            .ok_or("FREE pointer is missing")?;
        if pointer.kind != TokenKind::Word || is_grammar_keyword(pointer) {
            return Err("FREE operand must be a pointer identifier");
        }
        cursor.operand()?;
        pointers += 1;
    }
    (pointers > 0)
        .then_some(())
        .ok_or("FREE requires at least one pointer")
}

fn validate_compute(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let equal = find_punctuation(tokens, 1, "=").ok_or("COMPUTE requires exactly one equals")?;
    if find_punctuation(tokens, equal + 1, "=").is_some() {
        return Err("COMPUTE repeats equals");
    }
    let mut targets = &tokens[1..equal];
    if targets.last().is_some_and(|token| token.is("ROUNDED")) {
        targets = &targets[..targets.len() - 1];
    }
    validate_operand_list(targets, 1)?;
    if !validate_arithmetic_expression(&tokens[equal + 1..]) {
        return Err("COMPUTE expression is malformed");
    }
    Ok(())
}

fn validate_delete(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.expect("RECORD")?;
    cursor.finish()
}

fn validate_display(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let upon = find_word(tokens, 1, "UPON");
    let with = find_phrase(tokens, 1, &["WITH", "NO", "ADVANCING"]);
    if upon.is_some_and(|upon| with.is_some_and(|with| with < upon)) {
        return Err("DISPLAY phrases are reordered");
    }
    let operands_end = upon.or(with).unwrap_or(tokens.len());
    validate_operand_list(&tokens[1..operands_end], 1)?;
    let mut cursor = Cursor::new(tokens, operands_end);
    if cursor.eat("UPON") {
        cursor.operand()?;
    }
    if cursor.eat("WITH") {
        cursor.expect("NO")?;
        cursor.expect("ADVANCING")?;
    }
    cursor.finish()
}

fn validate_divide(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if !(cursor.eat("INTO") || cursor.eat("BY")) {
        return Err("DIVIDE requires INTO or BY");
    }
    cursor.operand()?;
    if cursor.eat("GIVING") {
        cursor.operand()?;
    }
    if cursor.eat("REMAINDER") {
        cursor.operand()?;
    }
    cursor.eat("ROUNDED");
    cursor.finish()
}

fn validate_entry(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    if tokens
        .get(1)
        .is_none_or(|token| token.kind != TokenKind::Literal)
    {
        return Err("ENTRY requires a literal name");
    }
    let mut cursor = Cursor::new(tokens, 2);
    if cursor.eat("USING") {
        let mut count = 0usize;
        while !cursor.done() {
            if cursor.at("USING") {
                return Err("USING is duplicated");
            }
            cursor.operand()?;
            count += 1;
        }
        if count == 0 {
            return Err("USING requires an argument");
        }
    }
    cursor.finish()
}

fn validate_exit(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    if tokens.len() == 1
        || (tokens.len() == 2
            && [
                "PROGRAM",
                "METHOD",
                "FUNCTION",
                "PERFORM",
                "PARAGRAPH",
                "SECTION",
            ]
            .iter()
            .any(|word| tokens[1].is(word)))
    {
        Ok(())
    } else {
        Err("invalid EXIT scope")
    }
}

fn validate_go_to(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    if let Some(depending) = find_word(tokens, 2, "DEPENDING") {
        validate_operand_list(&tokens[2..depending], 1)?;
        let mut cursor = Cursor::new(tokens, depending);
        cursor.expect("DEPENDING")?;
        cursor.expect("ON")?;
        cursor.operand()?;
        cursor.finish()
    } else {
        let mut cursor = Cursor::new(tokens, 2);
        cursor.operand()?;
        cursor.finish()
    }
}

fn validate_initialize(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let clause = find_any_word(tokens, 1, &["WITH", "REPLACING", "THEN"]).unwrap_or(tokens.len());
    validate_operand_list(&tokens[1..clause], 1)?;
    let mut cursor = Cursor::new(tokens, clause);
    if cursor.eat("WITH") {
        cursor.expect("FILLER")?;
    }
    if cursor.eat("REPLACING") {
        let categories = [
            "ALPHABETIC",
            "ALPHANUMERIC",
            "ALPHANUMERIC-EDITED",
            "DBCS",
            "EGCS",
            "NATIONAL",
            "NATIONAL-EDITED",
            "NUMERIC",
            "NUMERIC-EDITED",
            "UTF-8",
        ];
        let mut seen = BTreeSet::new();
        while !cursor.done() && !tokens[cursor.position].is("THEN") {
            let category = categories
                .iter()
                .find(|category| cursor.eat(category))
                .copied()
                .ok_or("REPLACING category is missing or invalid")?;
            if !seen.insert(category) {
                return Err("REPLACING category is duplicated");
            }
            cursor.eat("DATA");
            cursor.expect("BY")?;
            cursor.operand()?;
        }
        if seen.is_empty() {
            return Err("REPLACING category or value is missing");
        }
    }
    if cursor.eat("THEN") {
        cursor.expect("TO")?;
        cursor.expect("DEFAULT")?;
    }
    cursor.finish()
}

fn validate_inspect(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    let alternatives = ["TALLYING", "REPLACING", "CONVERTING"];
    let operation = alternatives.iter().find(|word| cursor.eat(word)).copied();
    let matched = usize::from(operation.is_some());
    if matched != 1 || cursor.done() || !validate_expression(&tokens[cursor.position..]) {
        return Err("INSPECT requires one operation and operands");
    }
    if tokens[cursor.position..]
        .iter()
        .any(|token| alternatives.iter().any(|word| token.is(word)))
    {
        return Err("INSPECT operation is duplicated or mutually exclusive");
    }
    let allowed = match operation {
        Some("TALLYING") => &[
            "FOR",
            "ALL",
            "LEADING",
            "CHARACTERS",
            "BEFORE",
            "AFTER",
            "INITIAL",
        ][..],
        Some("REPLACING") => &[
            "ALL",
            "LEADING",
            "FIRST",
            "CHARACTERS",
            "BY",
            "BEFORE",
            "AFTER",
            "INITIAL",
        ][..],
        Some("CONVERTING") => &["TO", "BEFORE", "AFTER", "INITIAL"][..],
        _ => &[],
    };
    if tokens[cursor.position..]
        .iter()
        .any(|token| is_grammar_keyword(token) && !allowed.iter().any(|word| token.is(word)))
    {
        return Err("INSPECT contains an unknown or misplaced phrase");
    }
    Ok(())
}

fn validate_generate(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 2);
    cursor.operand()?;
    cursor.expect("FROM")?;
    cursor.operand()?;
    if cursor.eat("COUNT") {
        if !cursor.eat("BYTES") {
            cursor.eat("CHARACTERS");
        }
        cursor.expect("IN")?;
        cursor.operand()?;
    }
    validate_json_names(&mut cursor)?;
    validate_json_suppress(&mut cursor)?;
    validate_json_converting(&mut cursor, false)?;
    cursor.finish()
}

fn validate_json_parse(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 2);
    cursor.operand()?;
    cursor.expect("INTO")?;
    cursor.operand()?;
    if cursor.eat("WITH") {
        cursor.expect("DETAIL")?;
    }
    validate_json_ignoring(&mut cursor)?;
    validate_json_names(&mut cursor)?;
    validate_json_suppress(&mut cursor)?;
    validate_json_converting(&mut cursor, true)?;
    cursor.finish()
}

fn validate_json_ignoring(cursor: &mut Cursor<'_>) -> Result<(), &'static str> {
    if !cursor.eat("IGNORING") {
        return Ok(());
    }
    loop {
        cursor.expect("JSON")?;
        cursor.expect("NULL")?;
        cursor.expect("FOR")?;
        if !cursor.eat("ALL") {
            cursor.operand()?;
        }
        if !cursor.eat("ALSO") {
            break;
        }
    }
    Ok(())
}

fn validate_json_names(cursor: &mut Cursor<'_>) -> Result<(), &'static str> {
    if !cursor.eat("NAME") {
        return Ok(());
    }
    let mut count = 0usize;
    while !cursor.done() && !cursor.at("SUPPRESS") && !cursor.at("CONVERTING") {
        cursor.eat("OF");
        cursor.operand()?;
        cursor.expect("IS")?;
        if !cursor.eat("OMITTED") {
            cursor.literal()?;
        }
        count += 1;
    }
    (count > 0)
        .then_some(())
        .ok_or("NAME requires a data item and replacement")
}

fn validate_json_suppress(cursor: &mut Cursor<'_>) -> Result<(), &'static str> {
    if !cursor.eat("SUPPRESS") {
        return Ok(());
    }
    let mut count = 0usize;
    while !cursor.done() && !cursor.at("CONVERTING") {
        cursor.operand()?;
        count += 1;
    }
    (count > 0)
        .then_some(())
        .ok_or("SUPPRESS requires a data item")
}

fn validate_json_converting(cursor: &mut Cursor<'_>, parsing: bool) -> Result<(), &'static str> {
    if !cursor.eat("CONVERTING") {
        return Ok(());
    }
    loop {
        cursor.operand()?;
        cursor.expect(if parsing { "FROM" } else { "TO" })?;
        cursor.expect("JSON")?;
        let boolean = cursor.eat("BOOLEAN") || cursor.eat("BOOL");
        if !boolean {
            cursor.expect("NULL")?;
        }
        cursor.expect("USING")?;
        cursor.json_conversion_value()?;
        if parsing && boolean {
            cursor.expect("AND")?;
            cursor.json_conversion_value()?;
        }
        if !cursor.eat("ALSO") {
            break;
        }
    }
    Ok(())
}

fn validate_xml_parse(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 2);
    cursor.operand()?;
    if cursor.eat("PROCESSING") {
        cursor.expect("PROCEDURE")?;
        cursor.operand()?;
        if cursor.eat("THROUGH") || cursor.eat("THRU") {
            cursor.operand()?;
        }
    } else if cursor.eat("INTO") {
        cursor.operand()?;
    } else {
        return Err("XML PARSE requires PROCESSING PROCEDURE");
    }
    cursor.finish()
}

fn validate_sort_merge(tokens: &[Token<'_>], sort: bool) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if cursor.eat("ON") {
        if !(cursor.eat("ASCENDING") || cursor.eat("DESCENDING")) {
            return Err("ON requires ASCENDING or DESCENDING");
        }
        cursor.expect("KEY")?;
        cursor.operand_list_until(&["USING", "INPUT"], 1)?;
    }
    if cursor.eat("USING") {
        cursor.operand_list_until(&["USING", "INPUT", "GIVING", "OUTPUT"], 1)?;
    } else if sort && cursor.eat("INPUT") {
        cursor.expect("PROCEDURE")?;
        cursor.operand()?;
        if cursor.eat("THROUGH") || cursor.eat("THRU") {
            cursor.operand()?;
        }
    } else {
        return Err("SORT or MERGE input route is missing");
    }
    if cursor.eat("GIVING") {
        cursor.operand_list(1)?;
    } else if sort && cursor.eat("OUTPUT") {
        cursor.expect("PROCEDURE")?;
        cursor.operand()?;
        if cursor.eat("THROUGH") || cursor.eat("THRU") {
            cursor.operand()?;
        }
    } else {
        return Err("SORT or MERGE output route is missing");
    }
    cursor.finish()
}

fn validate_move(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.eat("CORRESPONDING");
    cursor.operand()?;
    cursor.expect("TO")?;
    let mut receivers = 0usize;
    while !cursor.done() {
        if cursor.at("TO") {
            return Err("TO is duplicated");
        }
        cursor.operand()?;
        receivers += 1;
    }
    if receivers == 0 {
        return Err("MOVE receiver is missing");
    }
    cursor.finish()
}

fn validate_multiply(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.expect("BY")?;
    cursor.operand()?;
    if cursor.eat("GIVING") {
        cursor.operand()?;
    }
    cursor.eat("ROUNDED");
    cursor.finish()
}

fn validate_open(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    let mut groups = 0usize;
    while !cursor.done() {
        if !(cursor.eat("INPUT")
            || cursor.eat("OUTPUT")
            || cursor.eat("I-O")
            || cursor.eat("EXTEND"))
        {
            return Err("OPEN mode is missing or misplaced");
        }
        let start = cursor.position;
        while !cursor.done()
            && !["INPUT", "OUTPUT", "I-O", "EXTEND"]
                .iter()
                .any(|word| cursor.at(word))
        {
            cursor.operand()?;
        }
        if cursor.position == start {
            return Err("OPEN mode has no file");
        }
        groups += 1;
    }
    (groups > 0).then_some(()).ok_or("OPEN has no mode group")
}

fn validate_read(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    let sequential = if cursor.eat("NEXT") || cursor.eat("PREVIOUS") {
        cursor.expect("RECORD")?;
        true
    } else {
        cursor.eat("RECORD");
        false
    };
    if cursor.eat("INTO") {
        cursor.operand()?;
    }
    if cursor.eat("KEY") {
        if sequential {
            return Err("KEY cannot be combined with NEXT or PREVIOUS RECORD");
        }
        cursor.eat("IS");
        cursor.operand()?;
    }
    if cursor.eat("WITH") {
        if cursor.eat("NO") {
            cursor.expect("LOCK")?;
        } else {
            cursor.eat("KEPT");
            cursor.expect("LOCK")?;
        }
    } else if cursor.eat("IGNORE") {
        cursor.expect("LOCK")?;
    }
    if cursor.eat("NO") {
        cursor.expect("WAIT")?;
    } else {
        cursor.eat("WAIT");
    }
    cursor.finish()
}

fn validate_release(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if cursor.eat("FROM") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_return(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.expect("RECORD")?;
    if cursor.eat("INTO") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_rewrite(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if cursor.eat("FROM") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_search_header(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.eat("ALL");
    cursor.operand()?;
    if cursor.eat("VARYING") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_set(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let split = find_any_word(tokens, 1, &["TO", "UP", "DOWN"]).ok_or("SET action is missing")?;
    validate_operand_list(&tokens[1..split], 1)?;
    let mut cursor = Cursor::new(tokens, split);
    if cursor.eat("TO") {
        cursor.operand()?;
    } else if cursor.eat("UP") || cursor.eat("DOWN") {
        cursor.expect("BY")?;
        cursor.operand()?;
    } else {
        return Err("SET action is invalid");
    }
    cursor.finish()
}

fn validate_start(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    if cursor.eat("KEY") {
        cursor.eat("IS");
        if cursor.eat("NOT") {
            if !(cursor.eat("LESS") || cursor.eat("GREATER")) {
                return Err("invalid START relation");
            }
            cursor.eat("THAN");
        } else if cursor.eat("LESS") || cursor.eat("GREATER") {
            cursor.eat("THAN");
            cursor.eat("OR");
            cursor.eat("EQUAL");
            cursor.eat("TO");
        } else if cursor.eat("EQUAL") {
            cursor.eat("TO");
        } else if cursor
            .tokens
            .get(cursor.position)
            .is_some_and(|token| matches!(token.text, "=" | ">" | "<" | ">=" | "<="))
        {
            cursor.position += 1;
        } else {
            return Err("START KEY relation is missing");
        }
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_stop(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 2);
    if cursor.eat("RETURNING") {
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_string(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let into = find_word(tokens, 1, "INTO").ok_or("STRING requires INTO")?;
    let senders = &tokens[1..into];
    if !senders.iter().any(|token| token.is("DELIMITED")) {
        return Err("STRING sender delimiter is missing");
    }
    let mut position = 0usize;
    let mut count = 0usize;
    while position < senders.len() {
        position = consume_operand(senders, position).ok_or("invalid STRING sender")?;
        if senders
            .get(position)
            .is_some_and(|token| token.is("DELIMITED"))
        {
            position += 1;
            if senders.get(position).is_none_or(|token| !token.is("BY")) {
                return Err("DELIMITED requires BY");
            }
            position += 1;
            if senders.get(position).is_some_and(|token| token.is("SIZE")) {
                position += 1;
            } else {
                position =
                    consume_operand(senders, position).ok_or("delimiter value is missing")?;
            }
        }
        count += 1;
    }
    if count == 0 {
        return Err("STRING has no sender");
    }
    let mut cursor = Cursor::new(tokens, into + 1);
    cursor.operand()?;
    if cursor.eat("WITH") {
        cursor.expect("POINTER")?;
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_unstring(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    cursor.expect("DELIMITED")?;
    cursor.expect("BY")?;
    let into = cursor.find("INTO").ok_or("UNSTRING requires INTO")?;
    if into == cursor.position || !validate_expression(&tokens[cursor.position..into]) {
        return Err("UNSTRING delimiter is malformed");
    }
    cursor.position = into + 1;
    let clause = cursor
        .find_any(&["INTO", "WITH", "TALLYING"])
        .unwrap_or(tokens.len());
    validate_operand_list(&tokens[cursor.position..clause], 1)?;
    cursor.position = clause;
    if cursor.eat("WITH") {
        cursor.expect("POINTER")?;
        cursor.operand()?;
    }
    if cursor.eat("TALLYING") {
        cursor.expect("IN")?;
        cursor.operand()?;
    }
    cursor.finish()
}

fn validate_write(tokens: &[Token<'_>]) -> Result<(), &'static str> {
    let mut cursor = Cursor::new(tokens, 1);
    cursor.operand()?;
    let has_from = cursor.eat("FROM");
    if has_from || !cursor.done() && !cursor.at("AFTER") && !cursor.at("BEFORE") {
        cursor.operand()?;
    }
    if cursor.eat("AFTER") || cursor.eat("BEFORE") {
        cursor.eat("ADVANCING");
        if !cursor.done() {
            cursor.operand()?;
            cursor.eat("LINES");
        }
    }
    cursor.finish()
}

fn validate_perform_header(tokens: &[Token<'_>]) -> Result<PerformMode, &'static str> {
    if tokens.len() < 2 {
        return Err("PERFORM requires a procedure or inline control phrase");
    }
    let inline = tokens[1].is("WITH")
        || tokens[1].is("UNTIL")
        || tokens[1].is("VARYING")
        || tokens[1].kind == TokenKind::Number;
    if inline {
        let mut cursor = Cursor::new(tokens, 1);
        if cursor.eat("WITH") {
            cursor.expect("TEST")?;
            cursor.eat("BEFORE");
            cursor.eat("AFTER");
        }
        if cursor.eat("UNTIL") {
            if !validate_expression(&tokens[cursor.position..]) {
                return Err("PERFORM UNTIL condition is malformed");
            }
        } else if cursor.eat("VARYING") {
            cursor.operand()?;
            cursor.expect("FROM")?;
            cursor.operand()?;
            cursor.expect("BY")?;
            cursor.operand()?;
            cursor.expect("UNTIL")?;
            if !validate_expression(&tokens[cursor.position..]) {
                return Err("PERFORM VARYING condition is malformed");
            }
        } else {
            cursor.operand()?;
            cursor.expect("TIMES")?;
            cursor.finish()?;
        }
        Ok(PerformMode::Inline)
    } else {
        let mut cursor = Cursor::new(tokens, 1);
        cursor.operand()?;
        if cursor.eat("THROUGH") || cursor.eat("THRU") {
            cursor.operand()?;
        }
        if !cursor.done() {
            if cursor.eat("UNTIL") {
                if !validate_expression(&tokens[cursor.position..]) {
                    return Err("PERFORM UNTIL condition is malformed");
                }
            } else if cursor.eat("VARYING") {
                if !validate_expression(&tokens[cursor.position..]) {
                    return Err("PERFORM VARYING phrase is malformed");
                }
            } else {
                cursor.operand()?;
                cursor.expect("TIMES")?;
                cursor.finish()?;
            }
        }
        Ok(PerformMode::OutOfLine)
    }
}

fn validate_expression(tokens: &[Token<'_>]) -> bool {
    if tokens.is_empty()
        || adjust_balanced(tokens).is_err()
        || tokens
            .iter()
            .any(|token| classify(std::slice::from_ref(token), 0).is_some())
        || tokens.iter().any(|token| {
            token.kind == TokenKind::Punctuation
                && !matches!(
                    token.text,
                    "+" | "-"
                        | "*"
                        | "**"
                        | "/"
                        | "="
                        | ">"
                        | "<"
                        | ">="
                        | "<="
                        | "<>"
                        | "("
                        | ")"
                        | ","
                        | ":"
                )
        })
    {
        return false;
    }
    !tokens.last().is_some_and(|token| {
        token.kind == TokenKind::Punctuation
            && matches!(
                token.text,
                "+" | "-" | "*" | "/" | "=" | ">" | "<" | ">=" | "<="
            )
    })
}

fn validate_arithmetic_expression(tokens: &[Token<'_>]) -> bool {
    validate_expression(tokens)
        && !tokens.iter().any(|token| {
            PROCEDURE_STATEMENTS.iter().any(|descriptor| {
                descriptor
                    .grammar_keywords
                    .iter()
                    .any(|keyword| token.is(keyword))
            })
        })
}

fn validate_operand_list(tokens: &[Token<'_>], minimum: usize) -> Result<(), &'static str> {
    let mut position = 0usize;
    let mut count = 0usize;
    while position < tokens.len() {
        if tokens[position].text == "," {
            if count == 0 || position + 1 == tokens.len() {
                return Err("operand separator is misplaced");
            }
            position += 1;
            continue;
        }
        position = consume_operand(tokens, position).ok_or("operand is malformed")?;
        count += 1;
    }
    if count < minimum {
        Err("required operand is missing")
    } else {
        Ok(())
    }
}

fn consume_operand(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    let mut position = start;
    if tokens
        .get(position)
        .is_some_and(|token| matches!(token.text, "+" | "-"))
    {
        position += 1;
    }
    if tokens
        .get(position)
        .is_some_and(|token| token.is("FUNCTION"))
    {
        position += 1;
        if tokens.get(position)?.kind != TokenKind::Word {
            return None;
        }
        position += 1;
    } else if tokens
        .get(position)
        .is_some_and(|token| token.is("ADDRESS") || token.is("LENGTH"))
    {
        position += 1;
        if tokens.get(position).is_some_and(|token| token.is("OF")) {
            position += 1;
        }
        if !is_operand_atom(tokens.get(position)?) {
            return None;
        }
        position += 1;
    } else {
        if !is_operand_atom(tokens.get(position)?) {
            return None;
        }
        position += 1;
    }
    while position < tokens.len() {
        if tokens[position].text == "(" {
            let close = matching_parenthesis(tokens, position)?;
            if close == position + 1 {
                return None;
            }
            position = close + 1;
        } else if (tokens[position].is("OF") || tokens[position].is("IN"))
            && tokens
                .get(position + 1)
                .is_some_and(|token| token.kind == TokenKind::Word)
        {
            position += 2;
        } else {
            break;
        }
    }
    Some(position)
}

fn is_operand_atom(token: &Token<'_>) -> bool {
    matches!(
        token.kind,
        TokenKind::Word | TokenKind::Number | TokenKind::Literal
    ) && classify(std::slice::from_ref(token), 0).is_none()
        && !is_grammar_keyword(token)
        && !matches!(
            token.text.to_ascii_uppercase().as_str(),
            "ELSE" | "WHEN" | "THROUGH" | "THRU"
        )
}

fn is_grammar_keyword(token: &Token<'_>) -> bool {
    PROCEDURE_STATEMENTS.iter().any(|descriptor| {
        descriptor
            .grammar_keywords
            .iter()
            .any(|keyword| token.is(keyword))
    })
}

fn matching_parenthesis(tokens: &[Token<'_>], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if token.text == "(" {
            depth += 1;
        } else if token.text == ")" {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn adjust_balanced(tokens: &[Token<'_>]) -> Result<(), ()> {
    let mut depth = 0usize;
    for token in tokens {
        adjust_depth(token, &mut depth).map_err(|_| ())?;
    }
    if depth == 0 { Ok(()) } else { Err(()) }
}

fn adjust_depth(token: &Token<'_>, depth: &mut usize) -> Result<(), HirProblem> {
    if token.text == "(" {
        *depth = depth
            .checked_add(1)
            .ok_or(HirProblem::StatementLimitExceeded)?;
    } else if token.text == ")" {
        *depth = depth
            .checked_sub(1)
            .ok_or(HirProblem::MalformedToken(token.line))?;
    }
    Ok(())
}

fn find_word(tokens: &[Token<'_>], start: usize, word: &str) -> Option<usize> {
    find_any_word(tokens, start, &[word])
}

fn find_any_word(tokens: &[Token<'_>], start: usize, words: &[&str]) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if depth == 0 && words.iter().any(|word| token.is(word)) {
            return Some(index);
        }
        adjust_depth(token, &mut depth).ok()?;
    }
    None
}

fn find_phrase(tokens: &[Token<'_>], start: usize, phrase: &[&str]) -> Option<usize> {
    let mut depth = 0usize;
    for index in start..tokens.len() {
        if depth == 0 && phrase_at(tokens, index, phrase) {
            return Some(index);
        }
        adjust_depth(&tokens[index], &mut depth).ok()?;
    }
    None
}

fn find_punctuation(tokens: &[Token<'_>], start: usize, value: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        if depth == 0 && token.kind == TokenKind::Punctuation && token.text == value {
            return Some(index);
        }
        adjust_depth(token, &mut depth).ok()?;
    }
    None
}

fn phrase_at(tokens: &[Token<'_>], index: usize, phrase: &[&str]) -> bool {
    phrase.iter().enumerate().all(|(offset, word)| {
        tokens
            .get(index + offset)
            .is_some_and(|token| token.is(word))
    })
}

fn keyword_length(kind: StatementKind) -> usize {
    usize::from(matches!(
        kind,
        StatementKind::StopRun
            | StatementKind::GoTo
            | StatementKind::NextSentence
            | StatementKind::JsonGenerate
            | StatementKind::JsonParse
            | StatementKind::XmlGenerate
            | StatementKind::XmlParse
    )) + 1
}

struct Cursor<'a> {
    tokens: &'a [Token<'a>],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(tokens: &'a [Token<'a>], position: usize) -> Self {
        Self { tokens, position }
    }

    fn done(&self) -> bool {
        self.position == self.tokens.len()
    }

    fn at(&self, word: &str) -> bool {
        self.tokens
            .get(self.position)
            .is_some_and(|token| token.is(word))
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.at(word) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, word: &str) -> Result<(), &'static str> {
        if self.eat(word) {
            Ok(())
        } else {
            Err("required keyword is missing or reordered")
        }
    }

    fn operand(&mut self) -> Result<(), &'static str> {
        self.position =
            consume_operand(self.tokens, self.position).ok_or("operand is malformed")?;
        Ok(())
    }

    fn literal(&mut self) -> Result<(), &'static str> {
        if self
            .tokens
            .get(self.position)
            .is_some_and(|token| token.kind == TokenKind::Literal)
        {
            self.position += 1;
            Ok(())
        } else {
            Err("literal is missing")
        }
    }

    fn json_conversion_value(&mut self) -> Result<(), &'static str> {
        if [
            "SPACE",
            "SPACES",
            "ZERO",
            "ZEROES",
            "ZEROS",
            "LOW-VALUE",
            "LOW-VALUES",
            "HIGH-VALUE",
            "HIGH-VALUES",
        ]
        .iter()
        .any(|value| self.eat(value))
        {
            Ok(())
        } else {
            self.atom()
        }
    }

    fn atom(&mut self) -> Result<(), &'static str> {
        if self.tokens.get(self.position).is_some_and(is_operand_atom) {
            self.position += 1;
            Ok(())
        } else {
            Err("operand is malformed")
        }
    }

    fn operand_list(&mut self, minimum: usize) -> Result<(), &'static str> {
        validate_operand_list(&self.tokens[self.position..], minimum)?;
        self.position = self.tokens.len();
        Ok(())
    }

    fn operand_list_until(&mut self, words: &[&str], minimum: usize) -> Result<(), &'static str> {
        let end = self.find_any(words).unwrap_or(self.tokens.len());
        validate_operand_list(&self.tokens[self.position..end], minimum)?;
        self.position = end;
        Ok(())
    }

    fn find(&self, word: &str) -> Option<usize> {
        find_word(self.tokens, self.position, word)
    }

    fn find_any(&self, words: &[&str]) -> Option<usize> {
        find_any_word(self.tokens, self.position, words)
    }

    fn finish(&self) -> Result<(), &'static str> {
        if self.done() {
            Ok(())
        } else {
            Err("unknown suffix, duplicate phrase, or trailing token")
        }
    }
}
