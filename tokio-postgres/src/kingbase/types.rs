//! Kingbase-specific value wrappers.

use crate::types::{FromSql, IsNull, Kind, ToSql, Type};
use bytes::{BufMut, BytesMut};
use postgres_types::to_sql_checked;
use std::error::Error;
use std::fmt;

// ═══════════════════ [新增开始] Kingbase MySQL tinyint wrapper 类型支持 ═══════════════════
const SYS_SCHEMA: &str = "sys";
const TINYINT_NAME: &str = "tinyint";
const TINYINT_OID: u32 = 8100;
const YEAR_NAME: &str = "year";
const YEAR_OID: u32 = 7025;

/// Kingbase MySQL-compatible `sys.tinyint` value.
///
/// This wrapper is intentionally scoped to Kingbase compatibility support. It
/// does not register OID 8100 as a global PostgreSQL type and does not change
/// the primitive `i8` codec rules in `postgres-types`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TinyInt(pub i8);

impl From<i8> for TinyInt {
    fn from(value: i8) -> TinyInt {
        TinyInt(value)
    }
}

impl From<TinyInt> for i8 {
    fn from(value: TinyInt) -> i8 {
        value.0
    }
}

fn is_sys_tinyint(ty: &Type) -> bool {
    ty.oid() == TINYINT_OID
        && ty.name() == TINYINT_NAME
        && ty.schema() == SYS_SCHEMA
        && matches!(ty.kind(), Kind::Simple)
}

impl<'a> FromSql<'a> for TinyInt {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<TinyInt, Box<dyn Error + Sync + Send>> {
        if raw.len() != 1 {
            return Err("invalid buffer size".into());
        }

        Ok(TinyInt(i8::from_be_bytes([raw[0]])))
    }

    fn accepts(ty: &Type) -> bool {
        is_sys_tinyint(ty)
    }
}

impl ToSql for TinyInt {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_i8(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        is_sys_tinyint(ty)
    }

    to_sql_checked!();
}

/// Error returned when a value is outside Kingbase MySQL `YEAR` bounds.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct YearOutOfRange {
    value: i32,
}

impl YearOutOfRange {
    /// Returns the rejected value.
    pub fn value(self) -> i32 {
        self.value
    }
}

impl fmt::Display for YearOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "year value {} is outside the supported range {}..={}",
            self.value,
            Year::MIN,
            Year::MAX
        )
    }
}

impl Error for YearOutOfRange {}

/// Kingbase MySQL-compatible `sys.year` value.
///
/// The live Kingbase MySQL `YEAR` type is a `sys.year` domain over
/// `pg_catalog.int4` with range `1901..=2155`. This wrapper accepts only the
/// exact `sys.year` domain metadata and intentionally does not accept ordinary
/// `pg_catalog.int4`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Year(i32);

impl Year {
    /// Minimum supported Kingbase MySQL `YEAR` value.
    pub const MIN: i32 = 1901;

    /// Maximum supported Kingbase MySQL `YEAR` value.
    pub const MAX: i32 = 2155;

    /// Creates a `Year` after validating the Kingbase `sys.year` range.
    pub fn new(value: i32) -> Result<Year, YearOutOfRange> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Year(value))
        } else {
            Err(YearOutOfRange { value })
        }
    }

    /// Returns the underlying four-digit year value.
    pub fn get(self) -> i32 {
        self.0
    }
}

fn is_sys_year(ty: &Type) -> bool {
    ty.oid() == YEAR_OID
        && ty.name() == YEAR_NAME
        && ty.schema() == SYS_SCHEMA
        && matches!(ty.kind(), Kind::Domain(base) if base == &Type::INT4)
}

impl<'a> FromSql<'a> for Year {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Year, Box<dyn Error + Sync + Send>> {
        if raw.len() != 4 {
            return Err("invalid buffer size".into());
        }

        let value = i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]);
        Ok(Year::new(value)?)
    }

    fn accepts(ty: &Type) -> bool {
        is_sys_year(ty)
    }
}

impl ToSql for Year {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_i32(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        is_sys_year(ty)
    }

    to_sql_checked!();
}
// ═══════════════════ [新增结束] Kingbase MySQL tinyint wrapper 类型支持 ═══════════════════
