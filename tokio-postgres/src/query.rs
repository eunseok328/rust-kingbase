use crate::client::{InnerClient, Responses};
use crate::codec::FrontendMessage;
use crate::connection::RequestMessages;
// ═══════════════════ [新增开始] Kingbase query_typed read-side codec alias 需要连接模式 ═══════════════════
use crate::client::CompatibleMode;
use crate::kingbase::mysql_types;
// ═══════════════════ [新增结束] Kingbase query_typed read-side codec alias 需要连接模式 ═══════════════════
use crate::prepare::get_type;
// ═══════════════════ [修改开始] Kingbase parameter codec alias 需要保留 original WrongType ═══════════════════
// 原代码保留：
// use crate::types::{BorrowToSql, IsNull};
use crate::types::{BorrowToSql, IsNull, WrongType};
// ═══════════════════ [修改结束] Kingbase parameter codec alias 需要保留 original WrongType ═══════════════════
use crate::{Column, Error, Portal, Row, Statement};
use bytes::{Bytes, BytesMut};
use fallible_iterator::FallibleIterator;
use futures_util::Stream;
use log::{Level, debug, log_enabled};
use pin_project_lite::pin_project;
use postgres_protocol::message::backend::{CommandCompleteBody, Message};
use postgres_protocol::message::frontend;
// ═══════════════════ [修改开始] Kingbase parameter codec alias 预编码需要 ToSql trait object ═══════════════════
// 原代码保留：
// use postgres_types::Type;
use postgres_types::{ToSql, Type};
// ═══════════════════ [修改结束] Kingbase parameter codec alias 预编码需要 ToSql trait object ═══════════════════
use std::error::Error as StdError;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

struct BorrowToSqlParamsDebug<'a, T>(&'a [T]);

impl<T> fmt::Debug for BorrowToSqlParamsDebug<'_, T>
where
    T: BorrowToSql,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|x| x.borrow_to_sql()))
            .finish()
    }
}

pub async fn query<P, I>(
    client: &InnerClient,
    statement: Statement,
    params: I,
) -> Result<RowStream, Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = P>,
    I::IntoIter: ExactSizeIterator,
{
    let buf = if log_enabled!(Level::Debug) {
        let params = params.into_iter().collect::<Vec<_>>();
        debug!(
            "executing statement {} with parameters: {:?}",
            statement.name(),
            BorrowToSqlParamsDebug(params.as_slice()),
        );
        encode(client, &statement, params)?
    } else {
        encode(client, &statement, params)?
    };
    let responses = start(client, buf).await?;
    Ok(RowStream {
        statement,
        responses,
        rows_affected: None,
    })
}

pub async fn query_typed<P, I>(
    client: &Arc<InnerClient>,
    query: &str,
    params: I,
) -> Result<RowStream, Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = (P, Type)>,
{
    let buf = {
        let params = params.into_iter().collect::<Vec<_>>();
        let param_oids = params.iter().map(|(_, t)| t.oid()).collect::<Vec<_>>();

        client.with_buf(|buf| {
            frontend::parse("", query, param_oids, buf).map_err(Error::parse)?;
            // ═══════════════════ [修改开始] Kingbase query_typed parameter codec alias 临时计算 ═══════════════════
            // 原代码保留：
            // encode_bind_raw("", params, "", buf)?;
            let param_codec_types = params
                .iter()
                .map(|(_, type_)| param_codec_alias_for_mode(client.compatible_mode(), type_))
                .collect::<Vec<_>>();
            encode_bind_raw(
                "",
                params
                    .into_iter()
                    .zip(param_codec_types)
                    .map(|((param, type_), codec_type)| (param, type_, codec_type)),
                "",
                buf,
            )?;
            // ═══════════════════ [修改结束] Kingbase query_typed parameter codec alias 临时计算 ═══════════════════
            frontend::describe(b'S', "", buf).map_err(Error::encode)?;
            frontend::execute("", 0, buf).map_err(Error::encode)?;
            frontend::sync(buf);

            Ok(buf.split().freeze())
        })?
    };

    let mut responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    loop {
        match responses.next().await? {
            Message::ParseComplete | Message::BindComplete | Message::ParameterDescription(_) => {}
            Message::NoData => {
                return Ok(RowStream {
                    statement: Statement::unnamed(vec![], vec![]),
                    responses,
                    rows_affected: None,
                });
            }
            Message::RowDescription(row_description) => {
                let mut columns: Vec<Column> = vec![];
                let mut it = row_description.fields();
                while let Some(field) = it.next().map_err(Error::parse)? {
                    let type_ = get_type(client, field.type_oid()).await?;
                    // ═══════════════════ [新增开始] Kingbase MySQL read-side codec alias 写入 unnamed Column ═══════════════════
                    let codec_type = match client.compatible_mode() {
                        CompatibleMode::Mysql => {
                            mysql_types::read_codec_alias_for_mysql_type(&type_)
                        }
                        CompatibleMode::Pg | CompatibleMode::Oracle | CompatibleMode::SqlServer => {
                            None
                        }
                    };
                    // ═══════════════════ [新增结束] Kingbase MySQL read-side codec alias 写入 unnamed Column ═══════════════════
                    let column = Column {
                        name: field.name().to_string(),
                        table_oid: Some(field.table_oid()).filter(|n| *n != 0),
                        column_id: Some(field.column_id()).filter(|n| *n != 0),
                        type_modifier: field.type_modifier(),
                        r#type: type_,
                        // ═══════════════════ [新增开始] Kingbase MySQL read-side codec alias unnamed Column 字段 ═══════════════════
                        codec_type,
                        // ═══════════════════ [新增结束] Kingbase MySQL read-side codec alias unnamed Column 字段 ═══════════════════
                    };
                    columns.push(column);
                }
                return Ok(RowStream {
                    statement: Statement::unnamed(vec![], columns),
                    responses,
                    rows_affected: None,
                });
            }
            _ => return Err(Error::unexpected_message()),
        }
    }
}

pub async fn execute_typed<P, I>(
    client: &Arc<InnerClient>,
    query: &str,
    params: I,
) -> Result<u64, Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = (P, Type)>,
{
    let buf = {
        let params = params.into_iter().collect::<Vec<_>>();
        let param_oids = params.iter().map(|(_, t)| t.oid()).collect::<Vec<_>>();

        client.with_buf(|buf| {
            frontend::parse("", query, param_oids, buf).map_err(Error::parse)?;
            // ═══════════════════ [修改开始] Kingbase execute_typed parameter codec alias 临时计算 ═══════════════════
            // 原代码保留：
            // encode_bind_raw("", params, "", buf)?;
            let param_codec_types = params
                .iter()
                .map(|(_, type_)| param_codec_alias_for_mode(client.compatible_mode(), type_))
                .collect::<Vec<_>>();
            encode_bind_raw(
                "",
                params
                    .into_iter()
                    .zip(param_codec_types)
                    .map(|((param, type_), codec_type)| (param, type_, codec_type)),
                "",
                buf,
            )?;
            // ═══════════════════ [修改结束] Kingbase execute_typed parameter codec alias 临时计算 ═══════════════════
            frontend::describe(b'S', "", buf).map_err(Error::encode)?;
            frontend::execute("", 0, buf).map_err(Error::encode)?;
            frontend::sync(buf);

            Ok(buf.split().freeze())
        })?
    };

    let mut responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    let mut rows = 0;

    loop {
        match responses.next().await? {
            Message::ParseComplete
            | Message::BindComplete
            | Message::ParameterDescription(_)
            | Message::RowDescription(_) => {}
            Message::NoData => {
                rows = 0;
            }

            Message::DataRow(_) => {}
            Message::CommandComplete(body) => {
                rows = extract_row_affected(&body)?;
            }

            Message::EmptyQueryResponse => rows = 0,
            Message::ReadyForQuery(_) => return Ok(rows),
            _ => {
                return Err(Error::unexpected_message());
            }
        }
    }
}

pub async fn query_portal(
    client: &InnerClient,
    portal: &Portal,
    max_rows: i32,
) -> Result<RowStream, Error> {
    let buf = client.with_buf(|buf| {
        frontend::execute(portal.name(), max_rows, buf).map_err(Error::encode)?;
        frontend::sync(buf);
        Ok(buf.split().freeze())
    })?;

    let responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    Ok(RowStream {
        statement: portal.statement().clone(),
        responses,
        rows_affected: None,
    })
}

/// Extract the number of rows affected from [`CommandCompleteBody`].
pub fn extract_row_affected(body: &CommandCompleteBody) -> Result<u64, Error> {
    let rows = body
        .tag()
        .map_err(Error::parse)?
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap_or(0);
    Ok(rows)
}

pub async fn execute<P, I>(
    client: &InnerClient,
    statement: Statement,
    params: I,
) -> Result<u64, Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = P>,
    I::IntoIter: ExactSizeIterator,
{
    let buf = if log_enabled!(Level::Debug) {
        let params = params.into_iter().collect::<Vec<_>>();
        debug!(
            "executing statement {} with parameters: {:?}",
            statement.name(),
            BorrowToSqlParamsDebug(params.as_slice()),
        );
        encode(client, &statement, params)?
    } else {
        encode(client, &statement, params)?
    };
    let mut responses = start(client, buf).await?;

    let mut rows = 0;
    loop {
        match responses.next().await? {
            Message::DataRow(_) => {}
            Message::CommandComplete(body) => {
                rows = extract_row_affected(&body)?;
            }
            Message::EmptyQueryResponse => rows = 0,
            Message::ReadyForQuery(_) => return Ok(rows),
            _ => return Err(Error::unexpected_message()),
        }
    }
}

async fn start(client: &InnerClient, buf: Bytes) -> Result<Responses, Error> {
    let mut responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    match responses.next().await? {
        Message::BindComplete => {}
        _ => return Err(Error::unexpected_message()),
    }

    Ok(responses)
}

pub fn encode<P, I>(client: &InnerClient, statement: &Statement, params: I) -> Result<Bytes, Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = P>,
    I::IntoIter: ExactSizeIterator,
{
    client.with_buf(|buf| {
        encode_bind(statement, params, "", buf)?;
        frontend::execute("", 0, buf).map_err(Error::encode)?;
        frontend::sync(buf);
        Ok(buf.split().freeze())
    })
}

pub fn encode_bind<P, I>(
    statement: &Statement,
    params: I,
    portal: &str,
    buf: &mut BytesMut,
) -> Result<(), Error>
where
    P: BorrowToSql,
    I: IntoIterator<Item = P>,
    I::IntoIter: ExactSizeIterator,
{
    let params = params.into_iter();
    if params.len() != statement.params().len() {
        return Err(Error::parameters(params.len(), statement.params().len()));
    }

    // ═══════════════════ [修改开始] Kingbase prepared parameter codec alias 传入 bind 编码 ═══════════════════
    // 原代码保留：
    // encode_bind_raw(
    //     statement.name(),
    //     params.zip(statement.params().iter().cloned()),
    //     portal,
    //     buf,
    // )
    encode_bind_raw(
        statement.name(),
        params
            .zip(statement.params().iter().cloned())
            .zip(statement.param_codec_types().iter().cloned())
            .map(|((param, type_), codec_type)| (param, type_, codec_type)),
        portal,
        buf,
    )
    // ═══════════════════ [修改结束] Kingbase prepared parameter codec alias 传入 bind 编码 ═══════════════════
}

// ═══════════════════ [新增开始] Kingbase parameter codec alias 编码前选定类型并缓存 bytes ═══════════════════
struct EncodedParam {
    format: i16,
    value: Option<Bytes>,
}

type ToSqlError = Box<dyn StdError + Sync + Send>;

fn param_codec_alias_for_mode(mode: CompatibleMode, type_: &Type) -> Option<Type> {
    match mode {
        CompatibleMode::Mysql => mysql_types::param_codec_alias_for_mysql_type(type_),
        CompatibleMode::Pg | CompatibleMode::Oracle | CompatibleMode::SqlServer => None,
    }
}

fn value_bytes(is_null: IsNull, buf: BytesMut) -> Option<Bytes> {
    match is_null {
        IsNull::No => Some(buf.freeze()),
        IsNull::Yes => None,
    }
}

fn encode_param_with_type(param: &dyn ToSql, type_: &Type) -> Result<EncodedParam, ToSqlError> {
    let mut value = BytesMut::new();
    let is_null = param.to_sql_checked(type_, &mut value)?;

    Ok(EncodedParam {
        format: param.encode_format(type_) as i16,
        value: value_bytes(is_null, value),
    })
}

fn encode_param_with_alias(
    param: &dyn ToSql,
    original_type: &Type,
    codec_type: Option<&Type>,
) -> Result<EncodedParam, ToSqlError> {
    match encode_param_with_type(param, original_type) {
        Ok(encoded) => Ok(encoded),
        Err(original_error) if original_error.is::<WrongType>() => {
            if let Some(codec_type) = codec_type {
                match encode_param_with_type(param, codec_type) {
                    Ok(encoded) => Ok(encoded),
                    Err(codec_error) if codec_error.is::<WrongType>() => Err(original_error),
                    Err(codec_error) => Err(codec_error),
                }
            } else {
                Err(original_error)
            }
        }
        Err(original_error) => Err(original_error),
    }
}
// ═══════════════════ [新增结束] Kingbase parameter codec alias 编码前选定类型并缓存 bytes ═══════════════════

fn encode_bind_raw<P, I>(
    statement_name: &str,
    params: I,
    portal: &str,
    buf: &mut BytesMut,
) -> Result<(), Error>
where
    P: BorrowToSql,
    // ═══════════════════ [修改开始] Kingbase parameter codec alias bind 输入携带 original 与 codec type ═══════════════════
    // 原代码保留：
    // I: IntoIterator<Item = (P, Type)>,
    I: IntoIterator<Item = (P, Type, Option<Type>)>,
    // ═══════════════════ [修改结束] Kingbase parameter codec alias bind 输入携带 original 与 codec type ═══════════════════
    I::IntoIter: ExactSizeIterator,
{
    // ═══════════════════ [修改开始] Kingbase parameter codec alias 选择最终 bind type 并预编码 ═══════════════════
    // 原逻辑：
    // let (param_formats, params): (Vec<_>, Vec<_>) = params
    //     .into_iter()
    //     .map(|(p, ty)| (p.borrow_to_sql().encode_format(&ty) as i16, (p, ty)))
    //     .unzip();
    //
    // let mut error_idx = 0;
    let params = params.into_iter();
    let mut encoded_params = Vec::with_capacity(params.len());
    for (idx, (param, original_type, codec_type)) in params.enumerate() {
        let encoded =
            encode_param_with_alias(param.borrow_to_sql(), &original_type, codec_type.as_ref())
                .map_err(|e| Error::to_sql(e, idx))?;
        encoded_params.push(encoded);
    }

    let param_formats = encoded_params
        .iter()
        .map(|param| param.format)
        .collect::<Vec<_>>();
    // ═══════════════════ [修改结束] Kingbase parameter codec alias 选择最终 bind type 并预编码 ═══════════════════

    let r = frontend::bind(
        portal,
        statement_name,
        param_formats,
        encoded_params,
        |param, buf| {
            if let Some(value) = param.value {
                buf.extend_from_slice(&value);
                Ok(postgres_protocol::IsNull::No)
            } else {
                Ok(postgres_protocol::IsNull::Yes)
            }
        },
        Some(1),
        buf,
    );
    match r {
        Ok(()) => Ok(()),
        Err(frontend::BindError::Conversion(e)) => Err(Error::to_sql(e, 0)),
        Err(frontend::BindError::Serialization(e)) => Err(Error::encode(e)),
    }
}

pin_project! {
    /// A stream of table rows.
    #[project(!Unpin)]
    pub struct RowStream {
        statement: Statement,
        responses: Responses,
        rows_affected: Option<u64>,
    }
}

impl Stream for RowStream {
    type Item = Result<Row, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        loop {
            match ready!(this.responses.poll_next(cx)?) {
                Message::DataRow(body) => {
                    return Poll::Ready(Some(Ok(Row::new(this.statement.clone(), body)?)));
                }
                Message::CommandComplete(body) => {
                    *this.rows_affected = Some(extract_row_affected(&body)?);
                }
                Message::EmptyQueryResponse | Message::PortalSuspended => {}
                Message::ReadyForQuery(_) => return Poll::Ready(None),
                _ => return Poll::Ready(Some(Err(Error::unexpected_message()))),
            }
        }
    }
}

impl RowStream {
    /// Returns the number of rows affected by the query.
    ///
    /// This function will return `None` until the stream has been exhausted.
    pub fn rows_affected(&self) -> Option<u64> {
        self.rows_affected
    }
}

pub async fn sync(client: &InnerClient) -> Result<(), Error> {
    let buf = Bytes::from_static(b"S\0\0\0\x04");
    let mut responses = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)))?;

    match responses.next().await? {
        Message::ReadyForQuery(_) => Ok(()),
        _ => Err(Error::unexpected_message()),
    }
}
