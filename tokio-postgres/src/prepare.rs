use crate::client::InnerClient;
use crate::codec::FrontendMessage;
use crate::connection::RequestMessages;
use crate::error::SqlState;
use crate::types::{Field, Kind, Oid, Type};
use crate::{Column, Error, Statement};
use crate::{query, slice_iter};
use bytes::Bytes;
use fallible_iterator::FallibleIterator;
use futures_util::TryStreamExt;
use log::debug;
use postgres_protocol::message::backend::Message;
use postgres_protocol::message::frontend;
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const TYPEINFO_QUERY: &str = "\
SELECT t.typname, t.typtype, t.typelem, r.rngsubtype, t.typbasetype, n.nspname, t.typrelid
FROM pg_catalog.pg_type t
LEFT OUTER JOIN pg_catalog.pg_range r ON r.rngtypid = t.oid
INNER JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid
WHERE t.oid = $1
";

// Range types weren't added until Postgres 9.2, so pg_range may not exist
const TYPEINFO_FALLBACK_QUERY: &str = "\
SELECT t.typname, t.typtype, t.typelem, NULL::OID, t.typbasetype, n.nspname, t.typrelid
FROM pg_catalog.pg_type t
INNER JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid
WHERE t.oid = $1
";

const TYPEINFO_ENUM_QUERY: &str = "\
SELECT enumlabel
FROM pg_catalog.pg_enum
WHERE enumtypid = $1
ORDER BY enumsortorder
";

// Postgres 9.0 didn't have enumsortorder
const TYPEINFO_ENUM_FALLBACK_QUERY: &str = "\
SELECT enumlabel
FROM pg_catalog.pg_enum
WHERE enumtypid = $1
ORDER BY oid
";

const TYPEINFO_COMPOSITE_QUERY: &str = "\
SELECT attname, atttypid
FROM pg_catalog.pg_attribute
WHERE attrelid = $1
AND NOT attisdropped
AND attnum > 0
ORDER BY attnum
";

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct TypeInfo {
    name: String,
    type_: i8,
    elem_oid: Oid,
    rngsubtype: Option<Oid>,
    basetype: Oid,
    schema: String,
    relid: Oid,
}

pub async fn prepare(
    client: &Arc<InnerClient>,
    query: &str,
    types: &[Type],
) -> Result<Statement, Error> {
    let param_oids = types.iter().map(Type::oid).collect::<Vec<_>>();
    prepare_with_param_oids(client, query, &param_oids).await
}

pub(crate) async fn prepare_with_param_oids(
    client: &Arc<InnerClient>,
    query: &str,
    param_oids: &[Oid],
) -> Result<Statement, Error> {
    let name = format!("s{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
    let buf = encode(client, &name, query, param_oids)?;
    let mut responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    match responses.next().await? {
        Message::ParseComplete => {}
        _ => return Err(Error::unexpected_message()),
    }

    let parameter_description = match responses.next().await? {
        Message::ParameterDescription(body) => body,
        _ => return Err(Error::unexpected_message()),
    };

    let row_description = match responses.next().await? {
        Message::RowDescription(body) => Some(body),
        Message::NoData => None,
        _ => return Err(Error::unexpected_message()),
    };

    let mut parameters = vec![];
    let mut it = parameter_description.parameters();
    while let Some(oid) = it.next().map_err(Error::parse)? {
        let type_ = get_type(client, oid).await?;
        parameters.push(type_);
    }

    let mut columns = vec![];
    if let Some(row_description) = row_description {
        let mut it = row_description.fields();
        while let Some(field) = it.next().map_err(Error::parse)? {
            let type_ = get_type(client, field.type_oid()).await?;
            let column = Column {
                name: field.name().to_string(),
                table_oid: Some(field.table_oid()).filter(|n| *n != 0),
                column_id: Some(field.column_id()).filter(|n| *n != 0),
                type_modifier: field.type_modifier(),
                r#type: type_,
            };
            columns.push(column);
        }
    }

    Ok(Statement::new(client, name, parameters, columns))
}

fn prepare_rec<'a>(
    client: &'a Arc<InnerClient>,
    query: &'a str,
    types: &'a [Type],
) -> Pin<Box<dyn Future<Output = Result<Statement, Error>> + 'a + Send>> {
    Box::pin(prepare(client, query, types))
}

fn encode(
    client: &InnerClient,
    name: &str,
    query: &str,
    param_oids: &[Oid],
) -> Result<Bytes, Error> {
    if param_oids.is_empty() {
        debug!("preparing query {name}: {query}");
    } else {
        debug!("preparing query {name} with type OIDs {param_oids:?}: {query}");
    }

    let query = crate::sql_compat::rewrite_query(client.compatible_mode(), query);
    client.with_buf(|buf| {
        frontend::parse(name, &query, param_oids.iter().copied(), buf).map_err(Error::encode)?;
        frontend::describe(b'S', name, buf).map_err(Error::encode)?;
        frontend::sync(buf);
        Ok(buf.split().freeze())
    })
}

pub(crate) async fn get_type(client: &Arc<InnerClient>, oid: Oid) -> Result<Type, Error> {
    if oid == 0 {
        return Ok(Type::UNKNOWN);
    }

    let type_system = client.type_system();

    if let Some(type_) = client.type_(type_system, oid) {
        return Ok(type_);
    }

    // A compatibility overlay may intentionally reuse an OID occupied by a
    // PostgreSQL catalog type in another type system (for example SQL Server
    // nchar/money). Resolve the active mode's extension first; shared types
    // fall through to the single PostgreSQL table below.
    if let Some(type_) = Type::from_oid_in(type_system, oid) {
        if type_.type_system() == Some(type_system) && type_system != crate::types::TypeSystem::Pg {
            let info = load_type_info(client, oid)
                .await?
                .ok_or_else(|| type_catalog_error(&type_, "OID is absent from pg_type"))?;
            validate_extension_type(&type_, &info)?;
            client.set_type(type_system, oid, &type_);
        }
        return Ok(type_);
    }

    if let Some(type_) = Type::from_pg_oid(oid) {
        return Ok(type_);
    }

    let info = match load_type_info(client, oid).await? {
        Some(info) => info,
        None => return Err(Error::unexpected_message()),
    };

    let compatible_mode = client.compatible_mode();
    let kind = if info.type_ == b'e' as i8 {
        let variants = get_enum_variants(client, oid).await?;
        Kind::Enum(variants)
    } else if compatible_mode == crate::client::CompatibleMode::Mysql
        && info.type_ == b'l' as i8
        && info.name.starts_with("Enum_")
    {
        let variants = get_enum_variants(client, oid).await?;
        Kind::MySqlEnum(variants)
    } else if compatible_mode == crate::client::CompatibleMode::Mysql
        && info.type_ == b'y' as i8
        && info.name.starts_with("Set_")
    {
        Kind::MySqlSet
    } else if info.type_ == b'p' as i8 {
        Kind::Pseudo
    } else if info.basetype != 0 {
        let type_ = get_type_rec(client, info.basetype).await?;
        Kind::Domain(type_)
    } else if info.elem_oid != 0 {
        let type_ = get_type_rec(client, info.elem_oid).await?;
        Kind::Array(type_)
    } else if info.relid != 0 {
        let fields = get_composite_fields(client, info.relid).await?;
        Kind::Composite(fields)
    } else if let Some(rngsubtype) = info.rngsubtype {
        let type_ = get_type_rec(client, rngsubtype).await?;
        Kind::Range(type_)
    } else {
        Kind::Simple
    };

    let type_ = Type::new(info.name, oid, kind, info.schema);
    client.set_type(type_system, oid, &type_);

    Ok(type_)
}

async fn load_type_info(client: &Arc<InnerClient>, oid: Oid) -> Result<Option<TypeInfo>, Error> {
    let stmt = typeinfo_statement(client).await?;
    let mut rows = pin!(query::query(client, stmt, slice_iter(&[&oid])).await?);
    let Some(row) = rows.try_next().await? else {
        return Ok(None);
    };

    Ok(Some(TypeInfo {
        name: row.try_get(0)?,
        type_: row.try_get(1)?,
        elem_oid: row.try_get(2)?,
        rngsubtype: row.try_get(3)?,
        basetype: row.try_get(4)?,
        schema: row.try_get(5)?,
        relid: row.try_get(6)?,
    }))
}

fn validate_extension_type(expected: &Type, actual: &TypeInfo) -> Result<(), Error> {
    if expected.name() != actual.name || expected.schema() != actual.schema {
        return Err(type_catalog_error(
            expected,
            &format!("catalog contains {}.{} instead", actual.schema, actual.name),
        ));
    }

    let kind_matches = match expected.kind() {
        Kind::Simple => {
            actual.basetype == 0
                && actual.elem_oid == 0
                && actual.rngsubtype.is_none()
                && actual.relid == 0
        }
        Kind::Pseudo => actual.type_ == b'p' as i8,
        Kind::Domain(base) => actual.type_ == b'd' as i8 && actual.basetype == base.oid(),
        Kind::Array(member) => actual.elem_oid == member.oid(),
        Kind::Range(member) => actual.rngsubtype == Some(member.oid()),
        Kind::Enum(_) => actual.type_ == b'e' as i8,
        Kind::MySqlEnum(_) => actual.type_ == b'l' as i8,
        Kind::MySqlSet => actual.type_ == b'y' as i8,
        Kind::Multirange(_) => false,
        Kind::Composite(_) => actual.relid != 0,
        _ => false,
    };

    if !kind_matches {
        return Err(type_catalog_error(
            expected,
            &format!(
                "catalog kind differs (typtype={}, typelem={}, typbasetype={}, typrelid={})",
                actual.type_ as u8 as char, actual.elem_oid, actual.basetype, actual.relid
            ),
        ));
    }

    Ok(())
}

fn type_catalog_error(expected: &Type, detail: &str) -> Error {
    Error::config(
        format!(
            "Kingbase type catalog mismatch for {}.{} (OID {}): {detail}",
            expected.schema(),
            expected.name(),
            expected.oid()
        )
        .into(),
    )
}

fn get_type_rec<'a>(
    client: &'a Arc<InnerClient>,
    oid: Oid,
) -> Pin<Box<dyn Future<Output = Result<Type, Error>> + Send + 'a>> {
    Box::pin(get_type(client, oid))
}

async fn typeinfo_statement(client: &Arc<InnerClient>) -> Result<Statement, Error> {
    if let Some(stmt) = client.typeinfo() {
        return Ok(stmt);
    }

    let stmt = match prepare_rec(client, TYPEINFO_QUERY, &[]).await {
        Ok(stmt) => stmt,
        Err(ref e) if e.code() == Some(&SqlState::UNDEFINED_TABLE) => {
            prepare_rec(client, TYPEINFO_FALLBACK_QUERY, &[]).await?
        }
        Err(e) => return Err(e),
    };

    client.set_typeinfo(&stmt);
    Ok(stmt)
}

async fn get_enum_variants(client: &Arc<InnerClient>, oid: Oid) -> Result<Vec<String>, Error> {
    let stmt = typeinfo_enum_statement(client).await?;

    query::query(client, stmt, slice_iter(&[&oid]))
        .await?
        .and_then(|row| async move { row.try_get(0) })
        .try_collect()
        .await
}

async fn typeinfo_enum_statement(client: &Arc<InnerClient>) -> Result<Statement, Error> {
    if let Some(stmt) = client.typeinfo_enum() {
        return Ok(stmt);
    }

    let stmt = match prepare_rec(client, TYPEINFO_ENUM_QUERY, &[]).await {
        Ok(stmt) => stmt,
        Err(ref e) if e.code() == Some(&SqlState::UNDEFINED_COLUMN) => {
            prepare_rec(client, TYPEINFO_ENUM_FALLBACK_QUERY, &[]).await?
        }
        Err(e) => return Err(e),
    };

    client.set_typeinfo_enum(&stmt);
    Ok(stmt)
}

async fn get_composite_fields(client: &Arc<InnerClient>, oid: Oid) -> Result<Vec<Field>, Error> {
    let stmt = typeinfo_composite_statement(client).await?;

    let rows = query::query(client, stmt, slice_iter(&[&oid]))
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    let mut fields = vec![];
    for row in rows {
        let name = row.try_get(0)?;
        let oid = row.try_get(1)?;
        let type_ = get_type_rec(client, oid).await?;
        fields.push(Field::new(name, type_));
    }

    Ok(fields)
}

async fn typeinfo_composite_statement(client: &Arc<InnerClient>) -> Result<Statement, Error> {
    if let Some(stmt) = client.typeinfo_composite() {
        return Ok(stmt);
    }

    let stmt = prepare_rec(client, TYPEINFO_COMPOSITE_QUERY, &[]).await?;

    client.set_typeinfo_composite(&stmt);
    Ok(stmt)
}
