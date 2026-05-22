use crate::client::InnerClient;
use crate::codec::FrontendMessage;
use crate::connection::RequestMessages;
use crate::types::Type;
use postgres_protocol::message::frontend;
use std::sync::{Arc, Weak};

struct StatementInner {
    client: Weak<InnerClient>,
    name: String,
    params: Vec<Type>,
    // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 元数据 ═══════════════════
    param_codec_types: Vec<Option<Type>>,
    // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 元数据 ═══════════════════
    columns: Vec<Column>,
}

impl Drop for StatementInner {
    fn drop(&mut self) {
        if self.name.is_empty() {
            // Unnamed statements don't need to be closed
            return;
        }
        if let Some(client) = self.client.upgrade() {
            let buf = client.with_buf(|buf| {
                frontend::close(b'S', &self.name, buf).unwrap();
                frontend::sync(buf);
                buf.split().freeze()
            });
            let _ = client.send(RequestMessages::Single(FrontendMessage::Raw(buf)));
        }
    }
}

/// A prepared statement.
///
/// Prepared statements can only be used with the connection that created them.
#[derive(Clone)]
pub struct Statement(Arc<StatementInner>);

impl Statement {
    pub(crate) fn new(
        inner: &Arc<InnerClient>,
        name: String,
        params: Vec<Type>,
        // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 构造参数 ═══════════════════
        param_codec_types: Vec<Option<Type>>,
        // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 构造参数 ═══════════════════
        columns: Vec<Column>,
    ) -> Statement {
        // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 长度校验 ═══════════════════
        debug_assert_eq!(params.len(), param_codec_types.len());
        // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 长度校验 ═══════════════════
        Statement(Arc::new(StatementInner {
            client: Arc::downgrade(inner),
            name,
            params,
            // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 字段初始化 ═══════════════════
            param_codec_types,
            // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 字段初始化 ═══════════════════
            columns,
        }))
    }

    pub(crate) fn unnamed(params: Vec<Type>, columns: Vec<Column>) -> Statement {
        // ═══════════════════ [新增开始] Kingbase MySQL unnamed statement 默认无 parameter codec alias ═══════════════════
        let param_codec_types = vec![None; params.len()];
        // ═══════════════════ [新增结束] Kingbase MySQL unnamed statement 默认无 parameter codec alias ═══════════════════
        Statement(Arc::new(StatementInner {
            client: Weak::new(),
            name: String::new(),
            params,
            // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 字段初始化 ═══════════════════
            param_codec_types,
            // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 字段初始化 ═══════════════════
            columns,
        }))
    }

    pub(crate) fn name(&self) -> &str {
        &self.0.name
    }

    /// Returns the expected types of the statement's parameters.
    pub fn params(&self) -> &[Type] {
        &self.0.params
    }

    // ═══════════════════ [新增开始] Kingbase MySQL parameter codec alias 访问接口 ═══════════════════
    pub(crate) fn param_codec_types(&self) -> &[Option<Type>] {
        debug_assert_eq!(self.0.params.len(), self.0.param_codec_types.len());
        &self.0.param_codec_types
    }
    // ═══════════════════ [新增结束] Kingbase MySQL parameter codec alias 访问接口 ═══════════════════

    /// Returns information about the columns returned when the statement is queried.
    pub fn columns(&self) -> &[Column] {
        &self.0.columns
    }
}

impl std::fmt::Debug for Statement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("Statement")
            .field("name", &self.0.name)
            .field("params", &self.0.params)
            .field("columns", &self.0.columns)
            .finish_non_exhaustive()
    }
}

/// Information about a column of a query.
#[derive(Debug)]
pub struct Column {
    pub(crate) name: String,
    pub(crate) table_oid: Option<u32>,
    pub(crate) column_id: Option<i16>,
    pub(crate) type_modifier: i32,
    pub(crate) r#type: Type,
    // ═══════════════════ [新增开始] Kingbase read-side codec alias 元数据 ═══════════════════
    pub(crate) codec_type: Option<Type>,
    // ═══════════════════ [新增结束] Kingbase read-side codec alias 元数据 ═══════════════════
}

impl Column {
    /// Returns the name of the column.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the OID of the underlying database table.
    pub fn table_oid(&self) -> Option<u32> {
        self.table_oid
    }

    /// Return the column ID within the underlying database table.
    pub fn column_id(&self) -> Option<i16> {
        self.column_id
    }

    /// Return the type modifier
    pub fn type_modifier(&self) -> i32 {
        self.type_modifier
    }

    /// Returns the type of the column.
    pub fn type_(&self) -> &Type {
        &self.r#type
    }

    // ═══════════════════ [新增开始] Kingbase read-side codec alias 访问接口 ═══════════════════
    pub(crate) fn codec_type(&self) -> Option<&Type> {
        self.codec_type.as_ref()
    }
    // ═══════════════════ [新增结束] Kingbase read-side codec alias 访问接口 ═══════════════════
}
