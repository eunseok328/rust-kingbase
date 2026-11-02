use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io;

use crate::client::CompatibleMode;
use postgres_types::{Kind, Type};

/// SQL lexical settings, shared with the connection's ParameterStatus handler.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SqlParseSettings {
    standard_conforming_strings: bool,
    enable_unpaired_comment: bool,
}

impl Default for SqlParseSettings {
    fn default() -> Self {
        Self {
            standard_conforming_strings: true,
            enable_unpaired_comment: false,
        }
    }
}

impl SqlParseSettings {
    pub(crate) fn from_parameters(parameters: &HashMap<String, String>) -> Self {
        let mut settings = Self::default();
        for (name, value) in parameters {
            settings.update(name, value);
        }
        settings
    }

    pub(crate) fn update(&mut self, name: &str, value: &str) {
        match name {
            "standard_conforming_strings" => {
                self.standard_conforming_strings = !value.eq_ignore_ascii_case("off");
            }
            "enable_unpaired_comment" => {
                self.enable_unpaired_comment = value.eq_ignore_ascii_case("on");
            }
            _ => {}
        }
    }
}

/// Rewrite mode-specific placeholders and adapt typed SQL Server EXEC inputs.
pub(crate) fn rewrite_query<'a>(
    mode: CompatibleMode,
    settings: SqlParseSettings,
    query: &'a str,
    types: &[Type],
) -> io::Result<Cow<'a, str>> {
    if mode == CompatibleMode::Pg {
        return Ok(Cow::Borrowed(query));
    }

    let tokens = tokens(mode, settings, query);
    let oracle_reuses_names =
        mode == CompatibleMode::Oracle && oracle_reuses_parameter_names(query, &tokens);
    if mode == CompatibleMode::SqlServer
        && is_sqlserver_module_definition(&tokens)
        && tokens.iter().any(|t| matches!(t.kind, TokenKind::Named(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SQL Server module definitions use @name for procedure variables; send the DDL with batch_execute",
        ));
    }

    let declared = if mode == CompatibleMode::SqlServer {
        sqlserver_declared_names(&tokens)
    } else {
        HashSet::new()
    };
    // Reserve explicit PostgreSQL slots so mixed syntax never aliases them.
    let mut used = tokens
        .iter()
        .filter_map(|t| match t.kind {
            TokenKind::Explicit(index) => Some(index),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut names = HashMap::new();
    let mut next = 1;
    let mut output = None::<String>;
    let mut last = 0;
    let mut in_exec = false;

    for (index, token) in tokens.iter().enumerate() {
        let slot = match token.kind {
            TokenKind::Word(word) if mode == CompatibleMode::SqlServer => {
                if word.eq_ignore_ascii_case("EXEC") || word.eq_ignore_ascii_case("EXECUTE") {
                    in_exec = true;
                } else if is_sqlserver_statement_start(word) {
                    in_exec = false;
                }
                continue;
            }
            TokenKind::Semicolon => {
                in_exec = false;
                continue;
            }
            TokenKind::Named(name) if mode == CompatibleMode::SqlServer => {
                let name = name.to_lowercase();
                if declared.contains(&name) {
                    continue;
                }
                let followed_by_equals = tokens
                    .get(index + 1)
                    .is_some_and(|next| matches!(next.kind, TokenKind::Equals));
                if in_exec && followed_by_equals {
                    // EXEC proc @argument = @external: the label is not a bind.
                    continue;
                }
                if followed_by_equals
                    && index > 0
                    && matches!(
                        tokens[index - 1].kind,
                        TokenKind::Word(word) if word.eq_ignore_ascii_case("SET") || word.eq_ignore_ascii_case("SELECT")
                    )
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "ambiguous SQL Server variable @{name}; declare it before assignment"
                        ),
                    ));
                }
                if let Some(slot) = names.get(&name) {
                    Some(*slot)
                } else {
                    let slot = next_parameter(&mut used, &mut next)?;
                    names.insert(name, slot);
                    Some(slot)
                }
            }
            TokenKind::Named(name) if oracle_reuses_names => {
                // Unquoted Oracle bind names fold ASCII case. Preserve other
                // characters: Unicode lowercasing can merge distinct KES names.
                let name = name.to_ascii_lowercase();
                if let Some(slot) = names.get(&name) {
                    Some(*slot)
                } else {
                    let slot = next_parameter(&mut used, &mut next)?;
                    names.insert(name, slot);
                    Some(slot)
                }
            }
            // Ordinary Oracle SQL consumes one slot per occurrence, including
            // repeated names and numeric labels. Only MySQL has Question tokens.
            TokenKind::Named(_) | TokenKind::Question => {
                Some(next_parameter(&mut used, &mut next)?)
            }
            TokenKind::EscapedQuestion => None,
            _ => continue,
        };

        let output = output.get_or_insert_with(|| String::with_capacity(query.len() + 8));
        output.push_str(&query[last..token.start]);
        if let Some(slot) = slot {
            output.push('$');
            output.push_str(&slot.to_string());
        } else {
            output.push('?');
        }
        last = token.end;
    }

    let rewritten = match output {
        Some(mut output) => {
            output.push_str(&query[last..]);
            Cow::Owned(output)
        }
        None => Cow::Borrowed(query),
    };

    if mode != CompatibleMode::SqlServer {
        return Ok(rewritten);
    }

    let tokens = self::tokens(mode, settings, &rewritten);
    let mut names = tokens
        .iter()
        .filter_map(|token| match token.kind {
            TokenKind::Named(name) => Some(name.to_lowercase()),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut variable_id = 0_u32;
    let mut output = String::new();
    let mut last = 0;
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        let is_exec = matches!(token.kind, TokenKind::Word(word)
            if word.eq_ignore_ascii_case("EXEC") || word.eq_ignore_ascii_case("EXECUTE"));
        // A qualified identifier can itself contain an EXEC keyword.
        if !is_exec || rewritten[..token.start].trim_end().ends_with('.') {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        let mut depth = 0_usize;
        while end < tokens.len() {
            match tokens[end].kind {
                TokenKind::Semicolon if depth == 0 => break,
                TokenKind::Word(word)
                    if depth == 0
                        && !rewritten[..tokens[end].start].trim_end().ends_with('.')
                        && is_sqlserver_exec_boundary(word) =>
                {
                    break;
                }
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                _ => {}
            }
            end += 1;
        }

        let mut arguments = Vec::new();
        for position in index + 1..end.saturating_sub(2) {
            if let (TokenKind::Named(_), TokenKind::Equals, TokenKind::Explicit(slot)) = (
                &tokens[position].kind,
                &tokens[position + 1].kind,
                &tokens[position + 2].kind,
            ) {
                // OUTPUT arguments must remain declared server-side variables.
                if tokens.get(position + 3).is_some_and(|next| {
                    matches!(next.kind, TokenKind::Word(word)
                        if word.eq_ignore_ascii_case("OUTPUT") || word.eq_ignore_ascii_case("OUT"))
                }) {
                    continue;
                }
                arguments.push((position + 2, *slot));
            }
        }
        if arguments.is_empty() {
            index = end;
            continue;
        }

        let statement_end = tokens.get(end).map_or(rewritten.len(), |token| token.start);
        output.push_str(&rewritten[last..token.start]);
        output.push_str("BEGIN\n");
        let mut variables = HashMap::new();
        for &(_, slot) in &arguments {
            if variables.contains_key(&slot) {
                continue;
            }
            let type_ = types.get(slot as usize - 1).filter(|type_| {
                **type_ != Type::UNKNOWN && !matches!(type_.kind(), Kind::Pseudo)
            }).ok_or_else(|| io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("SQL Server named EXEC argument ${slot} requires an explicit input type; use prepare_typed, query_typed, or execute_typed"),
            ))?;
            let variable = loop {
                variable_id = variable_id.checked_add(1).ok_or_else(parameter_overflow)?;
                let name = format!("__rust_kingbase_exec_{variable_id}");
                if names.insert(name.clone()) {
                    break format!("@{name}");
                }
            };
            output.push_str(&format!(
                "DECLARE {variable} {} = ${slot};\n",
                sqlserver_exec_variable_type(type_)?,
            ));
            variables.insert(slot, variable);
        }
        let mut previous = token.start;
        for (position, slot) in arguments {
            let argument = &tokens[position];
            output.push_str(&rewritten[previous..argument.start]);
            output.push_str(&variables[&slot]);
            previous = argument.end;
        }
        output.push_str(&rewritten[previous..statement_end]);
        // Terminate any trailing line comment before closing the generated block.
        output.push_str("\n; END\n");
        last = statement_end;
        index = end;
    }
    if last == 0 {
        Ok(rewritten)
    } else {
        output.push_str(&rewritten[last..]);
        Ok(Cow::Owned(output))
    }
}

#[cfg_attr(test, allow(dead_code))]
fn sqlserver_exec_variable_type(type_: &Type) -> io::Result<String> {
    // ParameterDescription does not carry a fixed-length type's modifier.
    // Guessing it here can truncate/pad values or change overload selection.
    if type_.schema() == "sys"
        && matches!(
            type_.name(),
            "nchar" | "binary" | "bpcharbyte" | "varcharbyte"
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "SQL Server named EXEC input type {}.{} requires an explicit length; bind through a declared local variable",
                type_.schema(),
                type_.name(),
            ),
        ));
    }
    let name = format!(
        "\"{}\".\"{}\"",
        type_.schema().replace('"', "\"\""),
        type_.name().replace('"', "\"\""),
    );
    if type_.schema() == "sys" && matches!(type_.name(), "nvarchar" | "varbinary") {
        // Preserve the original type/OID without T-SQL's default length of 1.
        Ok(format!("{name}(MAX)"))
    } else if type_.schema() == "sys" && matches!(type_.name(), "datetime2" | "time") {
        Ok(format!("{name}(7)"))
    } else {
        Ok(name)
    }
}

#[cfg_attr(test, allow(dead_code))]
fn is_sqlserver_exec_boundary(word: &str) -> bool {
    is_sqlserver_statement_start(word)
        || [
            "END",
            "ELSE",
            "DECLARE",
            "IF",
            "WHILE",
            "RETURN",
            "PRINT",
            "THROW",
            "RAISERROR",
            "BREAK",
            "CONTINUE",
            "GOTO",
            "WAITFOR",
        ]
        .iter()
        .any(|keyword| word.eq_ignore_ascii_case(keyword))
}

#[derive(Clone, Copy)]
enum TokenKind<'a> {
    Word(&'a str),
    Named(&'a str),
    Question,
    EscapedQuestion,
    Explicit(u32),
    Comma,
    Semicolon,
    LeftParen,
    RightParen,
    Equals,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    kind: TokenKind<'a>,
    start: usize,
    end: usize,
}

fn tokens(mode: CompatibleMode, settings: SqlParseSettings, query: &str) -> Vec<Token<'_>> {
    let bytes = query.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        // Prefixes are checked before identifiers, so an embedded apostrophe
        // never exposes placeholders inside an Oracle alternative literal.
        if mode == CompatibleMode::Oracle {
            let q_start = if matches!(bytes[i], b'q' | b'Q') {
                Some(i)
            } else if matches!(bytes[i], b'n' | b'N')
                && bytes.get(i + 1).is_some_and(|b| matches!(b, b'q' | b'Q'))
            {
                Some(i + 1)
            } else {
                None
            };
            if let Some(end) = q_start.and_then(|q| skip_oracle_q_quote(query, q)) {
                i = end;
                continue;
            }
        }
        if matches!(bytes[i], b'e' | b'E') && bytes.get(i + 1) == Some(&b'\'') {
            i = skip_single_quote(bytes, i + 1, true);
            continue;
        }

        let kind = match bytes[i] {
            b'\'' => {
                i = skip_single_quote(bytes, i, !settings.standard_conforming_strings);
                continue;
            }
            b'"' => {
                i = skip_quoted_identifier(bytes, i, b'"');
                continue;
            }
            b'\x60' if mode == CompatibleMode::Mysql => {
                i = skip_quoted_identifier(bytes, i, b'\x60');
                continue;
            }
            b'[' if mode == CompatibleMode::SqlServer => {
                i = skip_sqlserver_bracket(bytes, i);
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                // KES accepts -- without the whitespace required by MySQL.
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'#' if mode == CompatibleMode::Mysql => {
                // KES gives multi-character operators such as #>, #>> and #-
                // priority over hash comments. Keep their contents intact and
                // continue scanning for parameters after the operator.
                i = mysql_hash_operator_end(bytes, i)
                    .unwrap_or_else(|| skip_line_comment(bytes, i));
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(bytes, i, settings.enable_unpaired_comment);
                continue;
            }
            b'$' => {
                if let Some(end) = skip_dollar_quote(query, i) {
                    i = end;
                    continue;
                }
                let end = ascii_digits_end(bytes, i + 1);
                if let Some(index) = parse_positive_parameter(&query[i + 1..end]) {
                    i = end;
                    TokenKind::Explicit(index)
                } else {
                    i += 1;
                    continue;
                }
            }
            b'?' if mode == CompatibleMode::Mysql => {
                if bytes.get(i + 1) == Some(&b'?') {
                    i += 2;
                    TokenKind::EscapedQuestion
                } else {
                    i += 1;
                    TokenKind::Question
                }
            }
            b':' if mode == CompatibleMode::Oracle => {
                if bytes.get(i + 1) == Some(&b':') {
                    i += 2;
                    continue;
                }
                let name_start = i + 1;
                let end = identifier_end(query, name_start, mode);
                if end == name_start {
                    i += 1;
                    continue;
                }
                let name = &query[name_start..end];
                i = end;
                if bytes.get(end) == Some(&b'.')
                    && (name.eq_ignore_ascii_case("new") || name.eq_ignore_ascii_case("old"))
                {
                    continue;
                }
                TokenKind::Named(name)
            }
            b'@' if mode == CompatibleMode::SqlServer => {
                let name_start = i + 1;
                if bytes.get(name_start) == Some(&b'@') {
                    i = identifier_end(query, name_start + 1, mode).max(name_start + 1);
                    continue;
                }
                let end = identifier_end(query, name_start, mode);
                if end == name_start {
                    i += 1;
                    continue;
                }
                i = end;
                TokenKind::Named(&query[name_start..end])
            }
            b',' => {
                i += 1;
                TokenKind::Comma
            }
            b';' => {
                i += 1;
                TokenKind::Semicolon
            }
            b'(' => {
                i += 1;
                TokenKind::LeftParen
            }
            b')' => {
                i += 1;
                TokenKind::RightParen
            }
            b'=' => {
                i += 1;
                TokenKind::Equals
            }
            _ => {
                let ch = query[i..].chars().next().unwrap();
                if ch == '_' || ch.is_alphanumeric() {
                    i = identifier_end(query, i, mode);
                    TokenKind::Word(&query[start..i])
                } else {
                    i += ch.len_utf8();
                    continue;
                }
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: i,
        });
    }
    tokens
}

fn identifier_end(query: &str, start: usize, mode: CompatibleMode) -> usize {
    let mut end = start;
    for ch in query[start..].chars() {
        if ch == '_'
            || ch == '$'
            || ch.is_alphanumeric()
            || ch == '#' && mode != CompatibleMode::Mysql
            || ch == '@' && mode == CompatibleMode::SqlServer
        {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn oracle_reuses_parameter_names(query: &str, tokens: &[Token<'_>]) -> bool {
    for (index, token) in tokens.iter().enumerate() {
        // Anonymous blocks can start with one or more <<label>> declarations.
        if matches!(token.kind, TokenKind::Word(_))
            && query[..token.start].trim_end().ends_with("<<")
            && query[token.end..].trim_start().starts_with(">>")
        {
            continue;
        }
        return match token.kind {
            TokenKind::Word(word)
                if word.eq_ignore_ascii_case("BEGIN") || word.eq_ignore_ascii_case("CALL") =>
            {
                true
            }
            // A PL/SQL declaration section leads into BEGIN; a standalone SQL
            // DECLARE cursor must keep the ordinary positional binding rules.
            TokenKind::Word(word) if word.eq_ignore_ascii_case("DECLARE") => tokens[index + 1..]
                .iter()
                .any(|t| matches!(t.kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("BEGIN"))),
            _ => false,
        };
    }
    false
}

fn sqlserver_declared_names(tokens: &[Token<'_>]) -> HashSet<String> {
    let mut declared = HashSet::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("DECLARE")) {
            continue;
        }
        let mut expect_name = true;
        let mut depth = 0_usize;
        for token in &tokens[index + 1..] {
            if depth == 0
                && (matches!(token.kind, TokenKind::Semicolon)
                    || matches!(token.kind, TokenKind::Word(word) if is_sqlserver_statement_start(word)))
            {
                break;
            }
            if expect_name {
                if let TokenKind::Named(name) = token.kind {
                    declared.insert(name.to_lowercase());
                }
                expect_name = false;
            }
            match token.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => depth = depth.saturating_sub(1),
                TokenKind::Comma if depth == 0 => expect_name = true,
                _ => {}
            }
        }
    }
    declared
}

fn is_sqlserver_statement_start(word: &str) -> bool {
    [
        "SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "EXEC", "EXECUTE", "SET", "BEGIN",
    ]
    .iter()
    .any(|keyword| word.eq_ignore_ascii_case(keyword))
}

fn is_sqlserver_module_definition(tokens: &[Token<'_>]) -> bool {
    tokens.windows(2).any(|pair| {
        matches!(pair[0].kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("CREATE") || word.eq_ignore_ascii_case("ALTER"))
            && matches!(pair[1].kind, TokenKind::Word(word) if ["PROC", "PROCEDURE", "FUNCTION", "TRIGGER"].iter().any(|kind| word.eq_ignore_ascii_case(kind)))
    }) || tokens.windows(4).any(|part| {
        matches!(part[0].kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("CREATE"))
            && matches!(part[1].kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("OR"))
            && matches!(part[2].kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("ALTER"))
            && matches!(part[3].kind, TokenKind::Word(word) if ["PROC", "PROCEDURE", "FUNCTION", "TRIGGER"].iter().any(|kind| word.eq_ignore_ascii_case(kind)))
    })
}

fn next_parameter(used: &mut HashSet<u32>, next: &mut u32) -> io::Result<u32> {
    while used.contains(next) {
        *next = next.checked_add(1).ok_or_else(parameter_overflow)?;
    }
    let index = *next;
    used.insert(index);
    *next = next.checked_add(1).ok_or_else(parameter_overflow)?;
    Ok(index)
}

fn parameter_overflow() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "parameter index overflow")
}

fn parse_positive_parameter(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|index| *index != 0)
}

fn ascii_digits_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    end
}

fn skip_single_quote(bytes: &[u8], start: usize, backslash_escapes: bool) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if backslash_escapes => i = (i + 2).min(bytes.len()),
            b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_oracle_q_quote(query: &str, start: usize) -> Option<usize> {
    let bytes = query.as_bytes();
    if bytes.get(start + 1) != Some(&b'\'') {
        return None;
    }
    let open = query.get(start + 2..)?.chars().next()?;
    let close = match open {
        '[' => ']',
        '{' => '}',
        '(' => ')',
        '<' => '>',
        other => other,
    };
    let content = start + 2 + open.len_utf8();
    let delimiter = format!("{close}'");
    Some(
        query[content..]
            .find(&delimiter)
            .map_or(query.len(), |offset| content + offset + delimiter.len()),
    )
}

fn skip_quoted_identifier(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
            } else {
                return i + 1;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn skip_sqlserver_bracket(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b']' {
            if bytes.get(i + 1) == Some(&b']') {
                i += 2;
            } else {
                return i + 1;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn mysql_hash_operator_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = start + 1;
    while let Some(byte) = bytes.get(end) {
        if !matches!(
            byte,
            b'+' | b'-'
                | b'*'
                | b'/'
                | b'<'
                | b'>'
                | b'='
                | b'~'
                | b'!'
                | b'@'
                | b'#'
                | b'%'
                | b'^'
                | b'&'
                | b'|'
                | b'?'
        ) || bytes[end..].starts_with(b"--")
            || bytes[end..].starts_with(b"/*")
        {
            break;
        }
        end += 1;
    }
    (end > start + 1).then_some(end)
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
        i += 1;
    }
    i
}

fn skip_block_comment(bytes: &[u8], start: usize, enable_unpaired_comment: bool) -> usize {
    let mut i = start + 2;
    let mut depth = 1;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            if !enable_unpaired_comment {
                depth += 1;
            }
            // KES consumes an inner opener as a token even without nesting.
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

fn skip_dollar_quote(query: &str, start: usize) -> Option<usize> {
    let mut end = start + 1;
    if query.as_bytes().get(end) != Some(&b'$') {
        let first = query.get(end..)?.chars().next()?;
        if first != '_' && !first.is_alphabetic() {
            return None;
        }
        for ch in query[end..].chars() {
            if ch == '_' || ch.is_alphanumeric() {
                end += ch.len_utf8();
            } else {
                break;
            }
        }
        if query.as_bytes().get(end) != Some(&b'$') {
            return None;
        }
    }
    let delimiter = &query[start..=end];
    let content = end + 1;
    Some(
        query[content..]
            .find(delimiter)
            .map_or(query.len(), |offset| content + offset + delimiter.len()),
    )
}
