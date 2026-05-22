use crate::types::{Kind, Oid, Type};

// ═══════════════════ [新增开始] Kingbase MySQL 模式类型映射层 ═══════════════════
const SYS_SCHEMA: &str = "sys";
// ═══════════════════ [新增开始] Kingbase MySQL date/time codec alias OID 常量 ═══════════════════
const KINGBASE_MYSQL_DATE_OID: Oid = 7944;
const KINGBASE_MYSQL_TIME_OID: Oid = 7950;
const KINGBASE_MYSQL_DATETIME_OID: Oid = 7952;
// ═══════════════════ [新增结束] Kingbase MySQL date/time codec alias OID 常量 ═══════════════════
// ═══════════════════ [新增开始] Kingbase MySQL binary/varbinary BYTEA codec alias OID 常量 ═══════════════════
const KINGBASE_MYSQL_BINARY_OID: Oid = 3383;
const KINGBASE_MYSQL_VARBINARY_OID: Oid = 3384;
// ═══════════════════ [新增结束] Kingbase MySQL binary/varbinary BYTEA codec alias OID 常量 ═══════════════════
// ═══════════════════ [新增开始] Kingbase MySQL domain parameter codec alias OID 常量 ═══════════════════
const KINGBASE_MYSQL_MEDIUMINT_OID: Oid = 7016;
const KINGBASE_MYSQL_LONGTEXT_OID: Oid = 7018;
const KINGBASE_MYSQL_MEDIUMTEXT_OID: Oid = 7019;
const KINGBASE_MYSQL_TINYTEXT_OID: Oid = 7020;
const KINGBASE_MYSQL_LONGBLOB_OID: Oid = 7021;
const KINGBASE_MYSQL_MEDIUMBLOB_OID: Oid = 7022;
const KINGBASE_MYSQL_TINYBLOB_OID: Oid = 7023;
const KINGBASE_MYSQL_JSON_OID: Oid = 7024;
const KINGBASE_MYSQL_BLOB_OID: Oid = 8013;
// ═══════════════════ [新增结束] Kingbase MySQL domain parameter codec alias OID 常量 ═══════════════════
const KINGBASE_MYSQL_TINYINT_OID: Oid = 8100;

#[derive(Debug, Copy, Clone)]
enum MysqlTypeStrategy {
    /// Kingbase MySQL 内置类型，但 mapping 层不直接拥有这个 Type。
    ///
    /// 先识别出来但不返回自定义 Type，让调用方继续走 Rust 原始的
    /// `Type::new(name, oid, kind, schema)` 路径，避免污染 postgres-types
    /// 或把 Kingbase 专用类型伪装成全局 PostgreSQL 内置类型。实际值支持
    /// 由 Kingbase-scoped wrapper 提供，例如 `kingbase::types::TinyInt`。
    CustomPending,
}

#[derive(Debug, Copy, Clone)]
struct MysqlTypeMapping {
    oid: Oid,
    name: &'static str,
    schema: &'static str,
    strategy: MysqlTypeStrategy,
}

const MYSQL_TYPE_MAPPINGS: &[MysqlTypeMapping] = &[MysqlTypeMapping {
    oid: KINGBASE_MYSQL_TINYINT_OID,
    name: "tinyint",
    schema: SYS_SCHEMA,
    strategy: MysqlTypeStrategy::CustomPending,
}];

/// Maps Kingbase MySQL-compatible types that need MySQL-specific client
/// behavior.
///
/// Return `Some(Type)` only for types that should be owned by the MySQL
/// compatibility layer. Return `None` for shared Kingbase/PostgreSQL types so
/// callers can fall back to the original PG type handling path.
pub(crate) fn map_mysql_type(name: &str, oid: Oid, kind: &Kind, schema: &str) -> Option<Type> {
    let mapping = MYSQL_TYPE_MAPPINGS.iter().find(|mapping| {
        mapping.oid == oid
            && mapping.name == name
            && mapping.schema == schema
            && matches!(kind, Kind::Simple)
    });

    match mapping.map(|mapping| mapping.strategy) {
        Some(MysqlTypeStrategy::CustomPending) | None => None,
    }
}

// ═══════════════════ [新增开始] Kingbase MySQL date/time read-side codec alias ═══════════════════
pub(crate) fn read_codec_alias_for_mysql_type(type_: &Type) -> Option<Type> {
    if type_.schema() != SYS_SCHEMA || !matches!(type_.kind(), Kind::Simple) {
        return None;
    }

    match (type_.oid(), type_.name()) {
        (KINGBASE_MYSQL_DATE_OID, "date") => Some(Type::DATE),
        (KINGBASE_MYSQL_TIME_OID, "time") => Some(Type::TIME),
        (KINGBASE_MYSQL_DATETIME_OID, "datetime") => Some(Type::TIMESTAMP),
        // ═══════════════════ [新增开始] Kingbase MySQL binary/varbinary read-side BYTEA codec alias ═══════════════════
        (KINGBASE_MYSQL_BINARY_OID, "binary") => Some(Type::BYTEA),
        (KINGBASE_MYSQL_VARBINARY_OID, "varbinary") => Some(Type::BYTEA),
        // ═══════════════════ [新增结束] Kingbase MySQL binary/varbinary read-side BYTEA codec alias ═══════════════════
        // ═══════════════════ [新增开始] Kingbase MySQL timestamp 暂不启用 read-side codec alias ═══════════════════
        // sys.timestamp OID 7954 在 live chrono 验证中保留了 metadata，
        // 但按 Type::TIMESTAMP 解码会出现 8 小时时区偏移。timestamp 的
        // timezone/codec 语义需要单独调研，在确认前不做 alias。
        // ═══════════════════ [新增结束] Kingbase MySQL timestamp 暂不启用 read-side codec alias ═══════════════════
        _ => None,
    }
}
// ═══════════════════ [新增结束] Kingbase MySQL date/time read-side codec alias ═══════════════════

// ═══════════════════ [新增开始] Kingbase MySQL domain parameter codec alias 判断工具 ═══════════════════
fn is_domain_over(type_: &Type, base_type: &Type) -> bool {
    match type_.kind() {
        Kind::Domain(base) => base == base_type,
        _ => false,
    }
}

fn is_sys_blob_domain(type_: &Type) -> bool {
    match type_.kind() {
        Kind::Domain(base) => {
            base.schema() == SYS_SCHEMA
                && base.oid() == KINGBASE_MYSQL_BLOB_OID
                && base.name() == "blob"
                && is_domain_over(base, &Type::BYTEA)
        }
        _ => false,
    }
}
// ═══════════════════ [新增结束] Kingbase MySQL domain parameter codec alias 判断工具 ═══════════════════

// ═══════════════════ [新增开始] Kingbase MySQL date/time/domain parameter codec alias ═══════════════════
pub(crate) fn param_codec_alias_for_mysql_type(type_: &Type) -> Option<Type> {
    // ═══════════════════ [修改开始] Kingbase MySQL parameter alias 支持 Simple 与 Domain 精确匹配 ═══════════════════
    // 原代码保留：
    // if type_.schema() != SYS_SCHEMA || !matches!(type_.kind(), Kind::Simple) {
    //     return None;
    // }
    if type_.schema() != SYS_SCHEMA {
        return None;
    }
    // ═══════════════════ [修改结束] Kingbase MySQL parameter alias 支持 Simple 与 Domain 精确匹配 ═══════════════════

    match (type_.oid(), type_.name()) {
        (KINGBASE_MYSQL_DATE_OID, "date") if matches!(type_.kind(), Kind::Simple) => {
            Some(Type::DATE)
        }
        (KINGBASE_MYSQL_TIME_OID, "time") if matches!(type_.kind(), Kind::Simple) => {
            Some(Type::TIME)
        }
        (KINGBASE_MYSQL_DATETIME_OID, "datetime") if matches!(type_.kind(), Kind::Simple) => {
            Some(Type::TIMESTAMP)
        }
        // ═══════════════════ [新增开始] Kingbase MySQL binary/varbinary parameter BYTEA codec alias ═══════════════════
        (KINGBASE_MYSQL_BINARY_OID, "binary") | (KINGBASE_MYSQL_VARBINARY_OID, "varbinary")
            if matches!(type_.kind(), Kind::Simple) =>
        {
            Some(Type::BYTEA)
        }
        // ═══════════════════ [新增结束] Kingbase MySQL binary/varbinary parameter BYTEA codec alias ═══════════════════
        // ═══════════════════ [新增开始] Kingbase MySQL verified sys domain parameter codec alias ═══════════════════
        (KINGBASE_MYSQL_MEDIUMINT_OID, "mediumint") if is_domain_over(type_, &Type::INT4) => {
            Some(Type::INT4)
        }
        (KINGBASE_MYSQL_TINYTEXT_OID, "tinytext")
        | (KINGBASE_MYSQL_MEDIUMTEXT_OID, "mediumtext")
        | (KINGBASE_MYSQL_LONGTEXT_OID, "longtext")
            if is_domain_over(type_, &Type::TEXT) =>
        {
            Some(Type::TEXT)
        }
        (KINGBASE_MYSQL_BLOB_OID, "blob") if is_domain_over(type_, &Type::BYTEA) => {
            Some(Type::BYTEA)
        }
        (KINGBASE_MYSQL_TINYBLOB_OID, "tinyblob")
        | (KINGBASE_MYSQL_MEDIUMBLOB_OID, "mediumblob")
        | (KINGBASE_MYSQL_LONGBLOB_OID, "longblob")
            if is_sys_blob_domain(type_) =>
        {
            Some(Type::BYTEA)
        }
        (KINGBASE_MYSQL_JSON_OID, "json") if is_domain_over(type_, &Type::JSONB) => {
            Some(Type::JSONB)
        }
        // ═══════════════════ [新增结束] Kingbase MySQL verified sys domain parameter codec alias ═══════════════════
        // ═══════════════════ [修改开始] Kingbase MySQL binary/varbinary 已启用参数 alias，保留其它类型禁用说明 ═══════════════════
        // 原注释保留：
        // sys.year OID 7025、sys.timestamp OID 7954、bit、binary/varbinary、
        // unsigned、enum/set 等类型不做参数 alias。
        // sys.year OID 7025、sys.timestamp OID 7954、bit、unsigned、enum/set
        // 等类型仍不做参数 alias。
        // ═══════════════════ [修改结束] Kingbase MySQL binary/varbinary 已启用参数 alias，保留其它类型禁用说明 ═══════════════════
        _ => None,
    }
}
// ═══════════════════ [新增结束] Kingbase MySQL date/time/domain parameter codec alias ═══════════════════
// ═══════════════════ [新增结束] Kingbase MySQL 模式类型映射层 ═══════════════════
