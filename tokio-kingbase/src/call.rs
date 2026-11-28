use crate::client::{CompatibleMode, InnerClient};
use crate::sql_compat::{self, SqlParseSettings};
use crate::types::{Oid, Type};
use crate::{Error, prepare, query};
use futures_util::TryStreamExt;
use std::sync::Arc;

const ARGUMENTS: &str = "
SELECT p.proallargtypes, p.proargmodes
FROM pg_catalog.pg_proc p
WHERE p.oid = pg_catalog.to_regproc($1)
  AND p.prokind = 'p'
  AND p.provariadic = 0
  AND p.proargmodes IS NOT NULL";

// Kingbase attaches parameter direction to the type OID sent in Parse.
const OUT: Oid = 1 << 30;
const INOUT: Oid = (1 << 29) | OUT;

pub(crate) async fn parameter_oids(
    client: &Arc<InnerClient>,
    name: &str,
    slots: Vec<usize>,
    oids: &mut Vec<Oid>,
) -> Result<(), Error> {
    let statement = prepare::prepare_raw(client, ARGUMENTS, &[Type::TEXT.oid()]).await?;
    let params: [&(dyn crate::types::ToSql + Sync); 1] = [&name];
    let mut rows = std::pin::pin!(query::query(client, statement, params).await?);
    let Some(row) = rows.try_next().await? else {
        return Ok(());
    };
    let types: Vec<Oid> = row.try_get(0)?;
    let modes: Vec<i8> = row.try_get(1)?;
    if slots.len() != types.len() || modes.len() != types.len() {
        return Ok(());
    }

    let mut bindings = vec![None; slots.len()];
    for ((slot, oid), mode) in slots.into_iter().zip(types).zip(modes) {
        let direction = match mode as u8 {
            b'i' => 0,
            b'o' => OUT,
            b'b' => INOUT,
            _ => return Ok(()),
        };
        let Some(binding) = bindings.get_mut(slot) else {
            return Ok(());
        };
        if binding.is_some_and(|previous| previous != (oid, direction)) {
            return Ok(());
        }
        *binding = Some((oid, direction));
    }

    for (slot, binding) in bindings.into_iter().enumerate() {
        if let Some((oid, direction)) = binding.filter(|&(_, direction)| direction != 0) {
            if slot >= oids.len() {
                oids.resize(slot + 1, 0);
            }
            if oids[slot] == 0 {
                oids[slot] = oid;
            }
            oids[slot] |= direction;
        }
    }
    Ok(())
}

// Only a standalone CALL with one placeholder per argument is eligible.
pub(crate) fn positional_call<'a>(
    client: &InnerClient,
    sql: &'a str,
) -> Option<(&'a str, Vec<usize>)> {
    let mode = client.compatible_mode();
    if mode == CompatibleMode::Pg {
        return None;
    }
    let settings = client.sql_parse_settings();
    let bytes = sql.as_bytes();
    let skip = |start| skip_layout(bytes, start, mode, settings);
    let start = skip(0);
    let end = sql_compat::identifier_end(sql, start, mode);
    if !sql[start..end].eq_ignore_ascii_case("CALL") {
        return None;
    }

    let start = skip(end);
    let mut end = routine_name_end(sql, start, mode)?;
    let dot = skip(end);
    if bytes.get(dot) == Some(&b'.') {
        let part = skip(dot + 1);
        if !sql[end..dot].trim().is_empty() || !sql[dot + 1..part].trim().is_empty() {
            return None;
        }
        end = routine_name_end(sql, part, mode)?;
    }
    let name = &sql[start..end];
    let open = skip(end);
    if bytes.get(open) != Some(&b'(') {
        return None;
    }

    let mut position = skip(open + 1);
    let mut slots = Vec::new();
    loop {
        if bytes.get(position) != Some(&b'$') {
            return None;
        }
        let start = position + 1;
        position = start;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        slots.push(sql[start..position].parse::<usize>().ok()?.checked_sub(1)?);
        position = skip(position);
        match bytes.get(position)? {
            b',' => position = skip(position + 1),
            b')' => break,
            _ => return None,
        }
    }
    position = skip(position + 1);
    if bytes.get(position) == Some(&b';') {
        position = skip(position + 1);
    }
    (position == bytes.len()).then_some((name, slots))
}

fn routine_name_end(sql: &str, start: usize, mode: CompatibleMode) -> Option<usize> {
    if sql.as_bytes().get(start) == Some(&b'"') {
        let end = sql_compat::skip_quoted_identifier(sql.as_bytes(), start, b'"');
        return (end < sql.len()).then_some(end);
    }
    let first = sql[start..].chars().next()?;
    (first == '_' || first.is_alphabetic()).then(|| sql_compat::identifier_end(sql, start, mode))
}

fn skip_layout(
    bytes: &[u8],
    mut i: usize,
    mode: CompatibleMode,
    settings: SqlParseSettings,
) -> usize {
    while let Some(&ch) = bytes.get(i) {
        i = match ch {
            ch if ch.is_ascii_whitespace() => i + 1,
            b'-' if bytes[i..].starts_with(b"--") => sql_compat::skip_line_comment(bytes, i),
            b'/' if bytes[i..].starts_with(b"/*") => {
                sql_compat::skip_block_comment(bytes, i, settings.unpaired_comments(mode))
            }
            b'#' if mode == CompatibleMode::Mysql
                && sql_compat::mysql_hash_operator_end(bytes, i).is_none() =>
            {
                sql_compat::skip_line_comment(bytes, i)
            }
            _ => break,
        };
    }
    i
}
