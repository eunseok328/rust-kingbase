use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io;

use crate::client::CompatibleMode;

pub(crate) fn rewrite_query(mode: CompatibleMode, query: &str) -> io::Result<Cow<'_, str>> {
    match mode {
        CompatibleMode::Mysql => Ok(rewrite_question_params(query)),
        CompatibleMode::SqlServer => rewrite_sqlserver_params(query),
        CompatibleMode::Oracle => Ok(rewrite_oracle_params(query)),
        CompatibleMode::Pg => Ok(Cow::Borrowed(query)),
    }
}

fn rewrite_question_params(query: &str) -> Cow<'_, str> {
    let bytes = query.as_bytes();
    let mut out = None::<String>;
    let mut last = 0;
    let mut param = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_single_quote(bytes, i),
            b'"' => i = skip_double_quote(bytes, i),
            b'`' => i = skip_backtick(bytes, i),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line_comment(bytes, i),
            b'#' => i = skip_line_comment(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block_comment(bytes, i),
            b'$' => i = skip_dollar_quote(bytes, i).unwrap_or(i + 1),
            b'?' => {
                let out = out.get_or_insert_with(|| String::with_capacity(query.len() + 8));
                out.push_str(&query[last..i]);

                if bytes.get(i + 1) == Some(&b'?') {
                    out.push('?');
                    i += 2;
                    last = i;
                } else {
                    param += 1;
                    out.push('$');
                    out.push_str(&param.to_string());
                    i += 1;
                    last = i;
                }
            }
            _ => i += 1,
        }
    }

    match out {
        Some(mut out) => {
            out.push_str(&query[last..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(query),
    }
}

#[derive(Clone, Copy)]
enum SqlServerTokenKind<'a> {
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
struct SqlServerToken<'a> {
    kind: SqlServerTokenKind<'a>,
    start: usize,
    end: usize,
}

fn sqlserver_tokens(query: &str) -> Vec<SqlServerToken<'_>> {
    let bytes = query.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let kind = match bytes[i] {
            b'\'' => {
                i = skip_sqlserver_single_quote(bytes, i);
                continue;
            }
            b'"' => {
                i = skip_double_quote(bytes, i);
                continue;
            }
            b'[' => {
                i = skip_sqlserver_bracket(bytes, i);
                continue;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                i = skip_line_comment(bytes, i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = skip_sqlserver_block_comment(bytes, i);
                continue;
            }
            b'$' => {
                if let Some(end) = skip_dollar_quote(bytes, i) {
                    i = end;
                    continue;
                }
                let end = ascii_digits_end(bytes, i + 1);
                if let Some(index) = parse_positive_param(&bytes[i + 1..end]) {
                    i = end;
                    SqlServerTokenKind::Explicit(index)
                } else {
                    i += 1;
                    continue;
                }
            }
            b'@' if bytes.get(i + 1) == Some(&b'@') => {
                i += 2;
                while bytes.get(i).is_some_and(|byte| is_identifier_cont(*byte)) {
                    i += 1;
                }
                continue;
            }
            b'@' if bytes
                .get(i + 1)
                .is_some_and(|byte| is_identifier_start(*byte)) =>
            {
                i += 2;
                while bytes.get(i).is_some_and(|byte| is_identifier_cont(*byte)) {
                    i += 1;
                }
                SqlServerTokenKind::Named(&query[start + 1..i])
            }
            b'?' if bytes.get(i + 1) == Some(&b'?') => {
                i += 2;
                SqlServerTokenKind::EscapedQuestion
            }
            b'?' => {
                i += 1;
                SqlServerTokenKind::Question
            }
            byte if is_identifier_start(byte) => {
                i += 1;
                while bytes.get(i).is_some_and(|byte| is_identifier_cont(*byte)) {
                    i += 1;
                }
                SqlServerTokenKind::Word(&query[start..i])
            }
            b',' => {
                i += 1;
                SqlServerTokenKind::Comma
            }
            b';' => {
                i += 1;
                SqlServerTokenKind::Semicolon
            }
            b'(' => {
                i += 1;
                SqlServerTokenKind::LeftParen
            }
            b')' => {
                i += 1;
                SqlServerTokenKind::RightParen
            }
            b'=' => {
                i += 1;
                SqlServerTokenKind::Equals
            }
            _ => {
                i += 1;
                continue;
            }
        };
        tokens.push(SqlServerToken {
            kind,
            start,
            end: i,
        });
    }
    tokens
}

fn sqlserver_declared_names<'a>(tokens: &[SqlServerToken<'a>]) -> HashSet<&'a str> {
    let mut declared = HashSet::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("DECLARE"))
        {
            continue;
        }
        let mut expect_name = true;
        let mut depth = 0_usize;
        for token in &tokens[index + 1..] {
            if depth == 0
                && (matches!(token.kind, SqlServerTokenKind::Semicolon)
                    || matches!(token.kind, SqlServerTokenKind::Word(word) if is_sqlserver_statement_start(word)))
            {
                break;
            }
            if expect_name {
                if let SqlServerTokenKind::Named(name) = token.kind {
                    declared.insert(name);
                }
                expect_name = false;
            }
            match token.kind {
                SqlServerTokenKind::LeftParen => depth += 1,
                SqlServerTokenKind::RightParen => depth = depth.saturating_sub(1),
                SqlServerTokenKind::Comma if depth == 0 => expect_name = true,
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

fn is_sqlserver_module_definition(tokens: &[SqlServerToken<'_>]) -> bool {
    tokens.windows(2).any(|pair| {
        matches!(pair[0].kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("CREATE") || word.eq_ignore_ascii_case("ALTER"))
            && matches!(pair[1].kind, SqlServerTokenKind::Word(word) if ["PROC", "PROCEDURE", "FUNCTION", "TRIGGER"].iter().any(|kind| word.eq_ignore_ascii_case(kind)))
    }) || tokens.windows(4).any(|part| {
        matches!(part[0].kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("CREATE"))
            && matches!(part[1].kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("OR"))
            && matches!(part[2].kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("ALTER"))
            && matches!(part[3].kind, SqlServerTokenKind::Word(word) if ["PROC", "PROCEDURE", "FUNCTION", "TRIGGER"].iter().any(|kind| word.eq_ignore_ascii_case(kind)))
    })
}

fn next_sqlserver_param(used: &mut HashSet<u32>, next: &mut u32) -> io::Result<u32> {
    while used.contains(next) {
        *next = next.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "parameter index overflow")
        })?;
    }
    let index = *next;
    used.insert(index);
    *next = next
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "parameter index overflow"))?;
    Ok(index)
}

fn rewrite_sqlserver_params(query: &str) -> io::Result<Cow<'_, str>> {
    let tokens = sqlserver_tokens(query);
    if is_sqlserver_module_definition(&tokens)
        && tokens
            .iter()
            .any(|token| matches!(token.kind, SqlServerTokenKind::Named(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SQL Server module definitions use @name for procedure variables; send the DDL with batch_execute",
        ));
    }
    let declared = sqlserver_declared_names(&tokens);
    let mut used = tokens
        .iter()
        .filter_map(|token| match token.kind {
            SqlServerTokenKind::Explicit(index) => Some(index),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut names = HashMap::new();
    let mut next = 1;
    let mut out = None::<String>;
    let mut last = 0;
    let mut in_exec = false;

    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            SqlServerTokenKind::Word(word) => {
                if word.eq_ignore_ascii_case("EXEC") || word.eq_ignore_ascii_case("EXECUTE") {
                    in_exec = true;
                } else if is_sqlserver_statement_start(word) {
                    in_exec = false;
                }
            }
            SqlServerTokenKind::Semicolon => in_exec = false,
            SqlServerTokenKind::Named(name) => {
                if declared
                    .iter()
                    .any(|local| local.eq_ignore_ascii_case(name))
                {
                    continue;
                }
                let followed_by_equals = tokens
                    .get(index + 1)
                    .is_some_and(|next| matches!(next.kind, SqlServerTokenKind::Equals));
                if in_exec && followed_by_equals {
                    continue;
                }
                if followed_by_equals
                    && tokens.get(index.wrapping_sub(1)).is_some_and(|previous| {
                        matches!(previous.kind, SqlServerTokenKind::Word(word) if word.eq_ignore_ascii_case("SET") || word.eq_ignore_ascii_case("SELECT"))
                    })
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("ambiguous SQL Server variable @{name}; declare it before assignment"),
                    ));
                }
                let slot = if let Some(slot) = names.get(name) {
                    *slot
                } else {
                    let slot = next_sqlserver_param(&mut used, &mut next)?;
                    names.insert(name, slot);
                    slot
                };
                let output = out.get_or_insert_with(|| String::with_capacity(query.len() + 8));
                output.push_str(&query[last..token.start]);
                output.push('$');
                output.push_str(&slot.to_string());
                last = token.end;
            }
            SqlServerTokenKind::Question | SqlServerTokenKind::EscapedQuestion => {
                let output = out.get_or_insert_with(|| String::with_capacity(query.len() + 8));
                output.push_str(&query[last..token.start]);
                if matches!(token.kind, SqlServerTokenKind::Question) {
                    let slot = next_sqlserver_param(&mut used, &mut next)?;
                    output.push('$');
                    output.push_str(&slot.to_string());
                } else {
                    output.push('?');
                }
                last = token.end;
            }
            _ => {}
        }
    }
    match out {
        Some(mut output) => {
            output.push_str(&query[last..]);
            Ok(Cow::Owned(output))
        }
        None => Ok(Cow::Borrowed(query)),
    }
}

fn skip_sqlserver_single_quote(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            if bytes.get(i + 1) == Some(&b'\'') {
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

fn skip_sqlserver_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
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

fn rewrite_oracle_params(query: &str) -> Cow<'_, str> {
    let bytes = query.as_bytes();
    let mut used_params = oracle_explicit_param_indexes(bytes);
    let mut named_params = HashMap::new();
    let mut next_param = 1;
    let mut out = None::<String>;
    let mut last = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_sqlserver_single_quote(bytes, i),
            b'q' | b'Q' => i = skip_oracle_q_quote(bytes, i).unwrap_or(i + 1),
            b'"' => i = skip_double_quote(bytes, i),
            b'`' => i = skip_backtick(bytes, i),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line_comment(bytes, i),
            b'#' => i = skip_line_comment(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block_comment(bytes, i),
            b'$' => i = skip_dollar_quote(bytes, i).unwrap_or(i + 1),
            b'?' => {
                let out = out.get_or_insert_with(|| String::with_capacity(query.len() + 8));
                out.push_str(&query[last..i]);

                if bytes.get(i + 1) == Some(&b'?') {
                    out.push('?');
                    i += 2;
                } else {
                    let param = next_oracle_param(&mut used_params, &mut next_param);
                    out.push('$');
                    out.push_str(&param.to_string());
                    i += 1;
                }
                last = i;
            }
            b':' if bytes.get(i + 1) == Some(&b':') => i += 2,
            b':' => {
                let Some((param_start, param_end)) = oracle_bind_name(bytes, i) else {
                    i += 1;
                    continue;
                };
                let name = &query[param_start..param_end];

                let param = if name.as_bytes().iter().all(u8::is_ascii_digit) {
                    let Some(param) = oracle_numeric_param(name) else {
                        i = param_end;
                        continue;
                    };
                    used_params.insert(param);
                    param
                } else if is_oracle_trigger_record(name) {
                    i = param_end;
                    continue;
                } else if let Some(param) = named_params.get(&name.to_ascii_lowercase()) {
                    *param
                } else {
                    let param = next_oracle_param(&mut used_params, &mut next_param);
                    named_params.insert(name.to_ascii_lowercase(), param);
                    param
                };

                {
                    let out = out.get_or_insert_with(|| String::with_capacity(query.len()));
                    out.push_str(&query[last..i]);
                    out.push('$');
                    out.push_str(&param.to_string());
                }
                i = param_end;
                last = i;
            }
            _ => i += 1,
        }
    }

    match out {
        Some(mut out) => {
            out.push_str(&query[last..]);
            Cow::Owned(out)
        }
        None => Cow::Borrowed(query),
    }
}

fn oracle_explicit_param_indexes(bytes: &[u8]) -> HashSet<u32> {
    let mut params = HashSet::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_sqlserver_single_quote(bytes, i),
            b'q' | b'Q' => i = skip_oracle_q_quote(bytes, i).unwrap_or(i + 1),
            b'"' => i = skip_double_quote(bytes, i),
            b'`' => i = skip_backtick(bytes, i),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line_comment(bytes, i),
            b'#' => i = skip_line_comment(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block_comment(bytes, i),
            b'$' => {
                if let Some(end) = skip_dollar_quote(bytes, i) {
                    i = end;
                } else {
                    let end = ascii_digits_end(bytes, i + 1);
                    if let Some(param) = parse_positive_param(&bytes[i + 1..end]) {
                        params.insert(param);
                    }
                    i = end.max(i + 1);
                }
            }
            b':' if bytes.get(i + 1) == Some(&b':') => i += 2,
            b':' => {
                if let Some((start, end)) = oracle_bind_name(bytes, i) {
                    if let Some(param) = parse_positive_param(&bytes[start..end]) {
                        params.insert(param);
                    }
                    i = end;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    params
}

fn oracle_bind_name(bytes: &[u8], colon: usize) -> Option<(usize, usize)> {
    let start = colon + 1;
    if !bytes
        .get(start)
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }

    let mut end = start + 1;
    while bytes.get(end).is_some_and(|byte| is_identifier_cont(*byte)) {
        end += 1;
    }
    Some((start, end))
}

fn oracle_numeric_param(name: &str) -> Option<u32> {
    parse_positive_param(name.as_bytes())
}

fn parse_positive_param(bytes: &[u8]) -> Option<u32> {
    (!bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit))
        .then(|| std::str::from_utf8(bytes).ok()?.parse().ok())
        .flatten()
        .filter(|param: &u32| *param != 0)
}

fn ascii_digits_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    end
}

fn next_oracle_param(used_params: &mut HashSet<u32>, next_param: &mut u32) -> u32 {
    while used_params.contains(next_param) {
        *next_param = next_param
            .checked_add(1)
            .expect("Oracle parameter index overflow");
    }

    let param = *next_param;
    used_params.insert(param);
    *next_param = next_param
        .checked_add(1)
        .expect("Oracle parameter index overflow");
    param
}

fn is_oracle_trigger_record(name: &str) -> bool {
    name.eq_ignore_ascii_case("new") || name.eq_ignore_ascii_case("old")
}

fn skip_single_quote(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_oracle_q_quote(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && is_identifier_cont(bytes[start - 1]) {
        return None;
    }
    if bytes.get(start + 1) != Some(&b'\'') {
        return None;
    }
    let open = *bytes.get(start + 2)?;
    let close = match open {
        b'[' => b']',
        b'{' => b'}',
        b'(' => b')',
        b'<' => b'>',
        other => other,
    };
    let mut i = start + 3;
    while i + 1 < bytes.len() {
        if bytes[i] == close && bytes[i + 1] == b'\'' {
            return Some(i + 2);
        }
        i += 1;
    }
    Some(bytes.len())
}

fn skip_double_quote(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if bytes.get(i + 1) == Some(&b'"') => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_backtick(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'`' if bytes.get(i + 1) == Some(&b'`') => i += 2,
            b'`' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_line_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\n' || bytes[i] == b'\r' {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_dollar_quote(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && is_identifier_cont(bytes[start - 1]) {
        return None;
    }

    let mut tag_end = start + 1;
    if tag_end >= bytes.len() {
        return None;
    }

    if bytes[tag_end] != b'$' {
        if !is_identifier_start(bytes[tag_end]) {
            return None;
        }
        tag_end += 1;
        while tag_end < bytes.len() && bytes[tag_end] != b'$' {
            if !is_identifier_cont(bytes[tag_end]) {
                return None;
            }
            tag_end += 1;
        }
        if tag_end >= bytes.len() {
            return None;
        }
    }

    let tag = &bytes[start..=tag_end];
    let mut i = tag_end + 1;
    while i + tag.len() <= bytes.len() {
        if &bytes[i..i + tag.len()] == tag {
            return Some(i + tag.len());
        }
        i += 1;
    }

    Some(bytes.len())
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_identifier_cont(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::{rewrite_oracle_params, rewrite_query, rewrite_question_params};
    use crate::client::CompatibleMode;

    #[test]
    fn rewrites_question_params() {
        assert_eq!(
            rewrite_question_params("select * from t where id = ?").as_ref(),
            "select * from t where id = $1"
        );
        assert_eq!(
            rewrite_question_params("select * from t where a = ? and b = ?").as_ref(),
            "select * from t where a = $1 and b = $2"
        );
    }

    #[test]
    fn skips_quoted_text_and_comments() {
        assert_eq!(
            rewrite_question_params("select '?' as x, col = ?").as_ref(),
            "select '?' as x, col = $1"
        );
        assert_eq!(
            rewrite_question_params("select `?` from t where id = ?").as_ref(),
            "select `?` from t where id = $1"
        );
        assert_eq!(
            rewrite_question_params("select ? /* ? */ -- ?\n and ?").as_ref(),
            "select $1 /* ? */ -- ?\n and $2"
        );
        assert_eq!(
            rewrite_question_params("select ? # ?\n and ?").as_ref(),
            "select $1 # ?\n and $2"
        );
    }

    #[test]
    fn handles_escaped_question_mark() {
        assert_eq!(
            rewrite_question_params("select ??, ?").as_ref(),
            "select ?, $1"
        );
    }

    #[test]
    fn skips_dollar_quotes() {
        assert_eq!(
            rewrite_question_params("select $$?$$, $tag$?$tag$, ?").as_ref(),
            "select $$?$$, $tag$?$tag$, $1"
        );
    }

    #[test]
    fn rewrites_oracle_positional_params() {
        assert_eq!(
            rewrite_oracle_params("select * from t where a = :1 and b = :12").as_ref(),
            "select * from t where a = $1 and b = $12"
        );
        assert_eq!(
            rewrite_oracle_params("select :2, :1 from dual").as_ref(),
            "select $2, $1 from dual"
        );
    }

    #[test]
    fn rewrites_oracle_named_and_jdbc_params() {
        assert_eq!(
            rewrite_oracle_params("select :id, :status, :id from dual").as_ref(),
            "select $1, $2, $1 from dual"
        );
        assert_eq!(
            rewrite_oracle_params("select ?, ?, ?? from dual").as_ref(),
            "select $1, $2, ? from dual"
        );
        assert_eq!(
            rewrite_oracle_params("select :2, :name, ?, :name, :1 from dual").as_ref(),
            "select $2, $3, $4, $3, $1 from dual"
        );
        assert_eq!(
            rewrite_oracle_params("select :ID, :id from dual").as_ref(),
            "select $1, $1 from dual"
        );
    }

    #[test]
    fn skips_non_oracle_bind_syntax() {
        assert_eq!(
            rewrite_oracle_params("select ':1', \"c:2\" from t -- :3\nwhere id = :4 /* :5 */")
                .as_ref(),
            "select ':1', \"c:2\" from t -- :3\nwhere id = $4 /* :5 */"
        );
        assert_eq!(
            rewrite_oracle_params("select $$:1$$, $tag$:2$tag$, :3").as_ref(),
            "select $$:1$$, $tag$:2$tag$, $3"
        );
        assert_eq!(
            rewrite_oracle_params("select value::text, :NEW.id, :OLD.id, :=, :0").as_ref(),
            "select value::text, :NEW.id, :OLD.id, :=, :0"
        );
        assert_eq!(
            rewrite_oracle_params("select q'[don't bind :hidden]', :id from dual").as_ref(),
            "select q'[don't bind :hidden]', $1 from dual"
        );
    }

    #[test]
    fn dispatches_mode_specific_parameter_rewrites() {
        assert_eq!(
            rewrite_query(CompatibleMode::Pg, "select ?")
                .unwrap()
                .as_ref(),
            "select ?"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::Mysql, "select ?")
                .unwrap()
                .as_ref(),
            "select $1"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::Oracle, "select :1")
                .unwrap()
                .as_ref(),
            "select $1"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::SqlServer, "select ?")
                .unwrap()
                .as_ref(),
            "select $1"
        );
    }

    #[test]
    fn sqlserver_named_parameters_reuse_slots() {
        let mode = CompatibleMode::SqlServer;
        assert_eq!(
            rewrite_query(mode, "SELECT @id, @id, @status")
                .unwrap()
                .as_ref(),
            "SELECT $1, $1, $2"
        );
        assert_eq!(
            rewrite_query(mode, "SELECT ?, @id, @id, ?")
                .unwrap()
                .as_ref(),
            "SELECT $1, $2, $2, $3"
        );
        assert_eq!(
            rewrite_query(mode, "SELECT @first, $2, @second")
                .unwrap()
                .as_ref(),
            "SELECT $1, $2, $3"
        );
    }

    #[test]
    fn sqlserver_skips_literals_identifiers_comments_and_system_variables() {
        let sql = "SELECT '@id', [@id], [a]]@id], \"@id\", @@ROWCOUNT, @id, ? /* @id /* ? */ @id */ FROM #tmp";
        assert_eq!(
            rewrite_query(CompatibleMode::SqlServer, sql)
                .unwrap()
                .as_ref(),
            "SELECT '@id', [@id], [a]]@id], \"@id\", @@ROWCOUNT, $1, $2 /* @id /* ? */ @id */ FROM #tmp"
        );
    }

    #[test]
    fn sqlserver_preserves_declared_variables_and_procedure_labels() {
        let mode = CompatibleMode::SqlServer;
        assert_eq!(
            rewrite_query(mode, "DECLARE @local INT = @seed; SELECT @local, @id, @id")
                .unwrap()
                .as_ref(),
            "DECLARE @local INT = $1; SELECT @local, $2, $2"
        );
        assert_eq!(
            rewrite_query(
                mode,
                "DECLARE @x INT = 1, @y DECIMAL(10,2) = @seed; SELECT @x, @y, @id"
            )
            .unwrap()
            .as_ref(),
            "DECLARE @x INT = 1, @y DECIMAL(10,2) = $1; SELECT @x, @y, $2"
        );
        assert_eq!(
            rewrite_query(mode, "EXEC dbo.proc @arg = @value, @other = @value")
                .unwrap()
                .as_ref(),
            "EXEC dbo.proc @arg = $1, @other = $1"
        );
        assert_eq!(
            rewrite_query(mode, "DECLARE @Local INT = 1; SELECT @local, @id")
                .unwrap()
                .as_ref(),
            "DECLARE @Local INT = 1; SELECT @local, $1"
        );
        assert!(rewrite_query(mode, "SET @unknown = @value").is_err());
        assert!(rewrite_query(mode, "CREATE PROCEDURE p @arg INT AS SELECT @arg").is_err());
    }
}
