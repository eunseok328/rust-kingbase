use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::client::CompatibleMode;

pub(crate) fn rewrite_query(mode: CompatibleMode, query: &str) -> Cow<'_, str> {
    match mode {
        CompatibleMode::Mysql | CompatibleMode::SqlServer => rewrite_question_params(query),
        CompatibleMode::Oracle => rewrite_oracle_params(query),
        CompatibleMode::Pg => Cow::Borrowed(query),
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
                } else if let Some(param) = named_params.get(name) {
                    *param
                } else {
                    let param = next_oracle_param(&mut used_params, &mut next_param);
                    named_params.insert(name, param);
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
            b'\'' => i = skip_single_quote(bytes, i),
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
    }

    #[test]
    fn dispatches_mode_specific_parameter_rewrites() {
        assert_eq!(
            rewrite_query(CompatibleMode::Pg, "select ?").as_ref(),
            "select ?"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::Mysql, "select ?").as_ref(),
            "select $1"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::Oracle, "select :1").as_ref(),
            "select $1"
        );
        assert_eq!(
            rewrite_query(CompatibleMode::SqlServer, "select ?").as_ref(),
            "select $1"
        );
    }
}
