//! KingbaseES-specific Rust type wrappers.

use bytes::{BufMut, BytesMut};
use std::error::Error;
use std::fmt;

use crate::{FromSql, IsNull, Kind, ToSql, Type};

/// A KingbaseES SQL Server-compatible unsigned `sys.tinyint` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerTinyInt(pub u8);

/// A KingbaseES MySQL-compatible `pg_catalog.jsonpath` value.
///
/// The PostgreSQL protocol represents JSONPATH in a server-defined binary
/// format. Obtain this payload from KingbaseES, for example by selecting a
/// `CAST(... AS JSONPATH)` expression, before binding it again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MySqlJsonPath(Vec<u8>);

impl MySqlJsonPath {
    /// Creates a JSONPATH value from its PostgreSQL-protocol binary payload.
    pub fn new(bytes: impl Into<Vec<u8>>) -> MySqlJsonPath {
        MySqlJsonPath(bytes.into())
    }

    /// Returns the PostgreSQL-protocol binary payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes this JSONPATH value and returns its PostgreSQL-protocol binary payload.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// A KingbaseES Oracle-compatible `pg_catalog.rowid` value.
///
/// The PostgreSQL protocol represents this value in binary form. Use
/// [`OracleRowId::as_bytes`] to inspect the opaque server-generated value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OracleRowId(Vec<u8>);

impl OracleRowId {
    /// Creates an Oracle ROWID from its PostgreSQL-protocol binary payload.
    pub fn new(bytes: impl Into<Vec<u8>>) -> OracleRowId {
        OracleRowId(bytes.into())
    }

    /// Returns the PostgreSQL-protocol binary payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes this ROWID and returns its PostgreSQL-protocol binary payload.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// A KingbaseES Oracle-compatible `sys.yminterval` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OracleYmInterval {
    months: i32,
}

impl OracleYmInterval {
    /// Creates a YEAR TO MONTH interval from its total number of months.
    pub fn new(months: i32) -> OracleYmInterval {
        OracleYmInterval { months }
    }

    /// Returns the interval's total number of months.
    pub fn months(self) -> i32 {
        self.months
    }
}

/// A KingbaseES Oracle-compatible `sys.dsinterval` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OracleDsInterval {
    days: i32,
    nanoseconds: i64,
}

impl OracleDsInterval {
    /// Creates a DAY TO SECOND interval from whole days and sub-day nanoseconds.
    pub fn new(days: i32, nanoseconds: i64) -> OracleDsInterval {
        OracleDsInterval { days, nanoseconds }
    }

    /// Returns the interval's whole-day component.
    pub fn days(self) -> i32 {
        self.days
    }

    /// Returns the interval's sub-day component in nanoseconds.
    pub fn nanoseconds(self) -> i64 {
        self.nanoseconds
    }
}

/// A PostgreSQL `INTERVAL` value.
///
/// PostgreSQL intervals retain independent microsecond, day, and month
/// components. This representation preserves those components without
/// converting calendar units into a fixed duration.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Interval {
    microseconds: i64,
    days: i32,
    months: i32,
}

impl Interval {
    /// Creates an interval from its microsecond, day, and month components.
    pub fn new(microseconds: i64, days: i32, months: i32) -> Interval {
        Interval {
            microseconds,
            days,
            months,
        }
    }

    /// Returns the microsecond component.
    pub fn microseconds(self) -> i64 {
        self.microseconds
    }

    /// Returns the day component.
    pub fn days(self) -> i32 {
        self.days
    }

    /// Returns the month component.
    pub fn months(self) -> i32 {
        self.months
    }
}

/// A KingbaseES SQL Server-compatible `sys.datetime2` value.
///
/// The PostgreSQL wire protocol stores the value as signed 100-nanosecond
/// ticks relative to `2000-01-01 00:00:00`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerDateTime2 {
    ticks: i64,
}

impl SqlServerDateTime2 {
    /// The number of DATETIME2 ticks in one second.
    pub const TICKS_PER_SECOND: i64 = 10_000_000;

    /// Creates a DATETIME2 value from signed 100-nanosecond ticks relative to
    /// `2000-01-01 00:00:00`.
    pub fn new(ticks: i64) -> SqlServerDateTime2 {
        SqlServerDateTime2 { ticks }
    }

    /// Returns the signed 100-nanosecond ticks relative to `2000-01-01 00:00:00`.
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// A KingbaseES SQL Server-compatible `sys.time` value.
///
/// The PostgreSQL wire protocol stores the value as unsigned-in-practice,
/// 100-nanosecond ticks since midnight.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerTime {
    ticks: i64,
}

impl SqlServerTime {
    /// The number of TIME ticks in one second.
    pub const TICKS_PER_SECOND: i64 = 10_000_000;

    const TICKS_PER_DAY: i64 = 24 * 60 * 60 * Self::TICKS_PER_SECOND;

    /// Creates a TIME value from 100-nanosecond ticks since midnight.
    pub fn new(ticks: i64) -> Result<SqlServerTime, SqlServerTimeError> {
        if !(0..Self::TICKS_PER_DAY).contains(&ticks) {
            return Err(SqlServerTimeError { ticks });
        }
        Ok(SqlServerTime { ticks })
    }

    /// Returns the 100-nanosecond ticks since midnight.
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// An error returned when SQL Server TIME ticks are outside one day.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SqlServerTimeError {
    ticks: i64,
}

impl SqlServerTimeError {
    /// Returns the invalid 100-nanosecond tick value.
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

impl fmt::Display for SqlServerTimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid SQL Server time ticks: {}", self.ticks)
    }
}

impl Error for SqlServerTimeError {}

/// A KingbaseES SQL Server-compatible `sys.money` value.
///
/// The PostgreSQL wire protocol stores the value as a signed integer scaled
/// by 10,000. This type preserves that exact scaled representation.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerMoney {
    scaled_value: i64,
}

impl SqlServerMoney {
    /// The number of stored units in one monetary unit.
    pub const SCALE: i64 = 10_000;

    /// Creates a MONEY value from its exact scaled integer representation.
    pub fn new(scaled_value: i64) -> SqlServerMoney {
        SqlServerMoney { scaled_value }
    }

    /// Returns the exact integer scaled by [`SqlServerMoney::SCALE`].
    pub fn scaled_value(self) -> i64 {
        self.scaled_value
    }
}

/// A KingbaseES SQL Server-compatible `sys.rowversion` value.
///
/// ROWVERSION is a server-generated, opaque eight-byte version token. It can
/// be supplied in comparison parameters but is not generated by this driver.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerRowVersion([u8; 8]);

impl SqlServerRowVersion {
    /// Creates a ROWVERSION value from its exact PostgreSQL-protocol payload.
    pub fn new(bytes: [u8; 8]) -> SqlServerRowVersion {
        SqlServerRowVersion(bytes)
    }

    /// Returns the exact server-generated version bytes.
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Consumes this value and returns the exact server-generated version bytes.
    pub fn into_bytes(self) -> [u8; 8] {
        self.0
    }
}

/// A KingbaseES SQL Server-compatible `sys.sql_variant` value.
///
/// SQL_VARIANT includes server-defined type metadata in its PostgreSQL-protocol
/// binary payload. Obtain this opaque payload from KingbaseES before binding it
/// again; this type does not convert Rust strings or numbers into SQL_VARIANT.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SqlServerVariant(Vec<u8>);

impl SqlServerVariant {
    /// Creates a SQL_VARIANT value from its PostgreSQL-protocol binary payload.
    pub fn new(bytes: impl Into<Vec<u8>>) -> SqlServerVariant {
        SqlServerVariant(bytes.into())
    }

    /// Returns the server-defined PostgreSQL-protocol binary payload.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes this value and returns its PostgreSQL-protocol binary payload.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// An error returned when decoding an Oracle interval wire value fails.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OracleIntervalError {
    /// The wire value is not PostgreSQL's 16-byte interval representation.
    InvalidWireLength {
        /// The actual payload length.
        actual: usize,
    },
    /// A YEAR TO MONTH interval contains day or time components.
    YearMonthContainsDayOrTime {
        /// The unexpected nanosecond component.
        nanoseconds: i64,
        /// The unexpected day component.
        days: i32,
    },
    /// A DAY TO SECOND interval contains a month component.
    DaySecondContainsMonths {
        /// The unexpected month component.
        months: i32,
    },
}

impl fmt::Display for OracleIntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            OracleIntervalError::InvalidWireLength { actual } => {
                write!(
                    f,
                    "invalid Oracle interval wire length: {actual}, expected 16"
                )
            }
            OracleIntervalError::YearMonthContainsDayOrTime { nanoseconds, days } => write!(
                f,
                "Oracle YEAR TO MONTH interval contains day/time components: days={days}, nanoseconds={nanoseconds}"
            ),
            OracleIntervalError::DaySecondContainsMonths { months } => {
                write!(f, "Oracle DAY TO SECOND interval contains months={months}")
            }
        }
    }
}

impl Error for OracleIntervalError {}

/// An error returned when constructing or decoding a [`MySqlBit`] value fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MySqlBitError {
    /// The logical bit length is outside MySQL BIT's supported range of 1 to 64 bits.
    InvalidBitLength {
        /// The rejected logical bit length.
        bit_len: u64,
    },
    /// The logical bit length exceeds the supported maximum of 64 bits.
    TooLong {
        /// The rejected logical bit length.
        bit_len: u64,
    },
    /// The payload length does not match the logical bit length.
    InvalidPayloadLength {
        /// The logical bit length.
        bit_len: u64,
        /// The required payload length in bytes.
        expected: usize,
        /// The actual payload length in bytes.
        actual: usize,
    },
    /// The numeric value cannot fit in the requested logical bit width.
    ValueTooLarge {
        /// The requested logical bit width.
        bit_len: u64,
        /// The rejected numeric value.
        value: u64,
    },
    /// The binary value is shorter than the 8-byte metadata header.
    MissingLengthHeader {
        /// The actual wire payload length in bytes.
        actual: usize,
    },
}

impl fmt::Display for MySqlBitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            MySqlBitError::InvalidBitLength { bit_len } => {
                write!(
                    f,
                    "bit length {bit_len} is outside the supported range of 1 to 64"
                )
            }
            MySqlBitError::TooLong { bit_len } => {
                write!(
                    f,
                    "bit length {bit_len} exceeds the supported maximum of 64"
                )
            }
            MySqlBitError::InvalidPayloadLength {
                bit_len,
                expected,
                actual,
            } => write!(
                f,
                "payload length {actual} does not match bit length {bit_len}; expected {expected}"
            ),
            MySqlBitError::ValueTooLarge { bit_len, value } => {
                write!(f, "value {value} does not fit in {bit_len} bits")
            }
            MySqlBitError::MissingLengthHeader { actual } => {
                write!(
                    f,
                    "bit wire payload has {actual} bytes, expected at least 8"
                )
            }
        }
    }
}

impl Error for MySqlBitError {}

/// An error returned when constructing or decoding a [`MySqlTime`] value
/// outside KingbaseES MySQL TIME's supported range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MySqlTimeError {
    micros: i64,
}

impl MySqlTimeError {
    /// Returns the rejected signed microsecond duration.
    pub fn micros(self) -> i64 {
        self.micros
    }
}

impl fmt::Display for MySqlTimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mysql time duration is out of range: {} microseconds",
            self.micros
        )
    }
}

impl Error for MySqlTimeError {}

/// A KingbaseES MySQL-compatible `sys.time` duration.
///
/// MySQL TIME is a signed duration rather than a time of day. Its valid range
/// is `-838:59:59.999999` through `838:59:59.999999`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MySqlTime {
    micros: i64,
}

impl MySqlTime {
    /// The largest magnitude MySQL TIME duration, in microseconds.
    pub const MAX_MICROS: i64 = 3_020_399_999_999;

    /// Creates a MySQL TIME duration from signed microseconds.
    pub fn new(micros: i64) -> Result<MySqlTime, MySqlTimeError> {
        if !(-Self::MAX_MICROS..=Self::MAX_MICROS).contains(&micros) {
            return Err(MySqlTimeError { micros });
        }
        Ok(MySqlTime { micros })
    }

    /// Returns the signed duration in microseconds.
    pub fn micros(self) -> i64 {
        self.micros
    }
}

/// A KingbaseES MySQL-compatible `sys.bit` value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MySqlBit {
    bit_len: u64,
    payload: Vec<u8>,
}

impl MySqlBit {
    /// Creates a bit value from a logical bit length and MSB-first payload bytes.
    pub fn new(bit_len: u64, payload: impl Into<Vec<u8>>) -> Result<MySqlBit, MySqlBitError> {
        let payload = payload.into();
        let expected = bit_payload_len(bit_len)?;
        if payload.len() != expected {
            return Err(MySqlBitError::InvalidPayloadLength {
                bit_len,
                expected,
                actual: payload.len(),
            });
        }

        Ok(MySqlBit {
            bit_len,
            payload: normalize_bit_payload(bit_len, payload),
        })
    }

    /// Creates a bit value from an unsigned integer.
    ///
    /// The integer is represented as a logical MSB-first bit string of
    /// exactly `bit_len` bits. This is distinct from [`MySqlBit::new`]'s
    /// payload-oriented API and is convenient when the SQL value is treated
    /// as a number rather than as an opaque bit string.
    pub fn from_u64(bit_len: u64, value: u64) -> Result<MySqlBit, MySqlBitError> {
        let payload_len = bit_payload_len(bit_len)?;
        if bit_len < 64 && value >= (1_u64 << bit_len) {
            return Err(MySqlBitError::ValueTooLarge { bit_len, value });
        }

        let mut payload = vec![0; payload_len];
        for index in 0..bit_len as usize {
            if value & (1_u64 << (bit_len as usize - 1 - index)) != 0 {
                payload[index / 8] |= 0x80 >> (index % 8);
            }
        }
        Ok(MySqlBit { bit_len, payload })
    }

    /// Creates a one-bit value from a boolean.
    pub fn from_bool(value: bool) -> MySqlBit {
        MySqlBit {
            bit_len: 1,
            payload: vec![if value { 0x80 } else { 0x00 }],
        }
    }

    /// Returns the logical bit length.
    pub fn bit_len(&self) -> u64 {
        self.bit_len
    }

    /// Returns the normalized MSB-first payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns the logical bit string as an unsigned integer.
    pub fn to_u64(&self) -> u64 {
        let mut value = 0;
        for index in 0..self.bit_len as usize {
            if self.payload[index / 8] & (0x80 >> (index % 8)) != 0 {
                value |= 1_u64 << (self.bit_len as usize - 1 - index);
            }
        }
        value
    }

    /// Returns this value as a boolean when its logical bit length is exactly 1.
    pub fn as_bool(&self) -> Option<bool> {
        if self.bit_len == 1 {
            Some(self.payload.first().copied().unwrap_or(0) & 0x80 != 0)
        } else {
            None
        }
    }
}

fn bit_payload_len(bit_len: u64) -> Result<usize, MySqlBitError> {
    if bit_len == 0 {
        return Err(MySqlBitError::InvalidBitLength { bit_len });
    }
    if bit_len > 64 {
        return Err(MySqlBitError::TooLong { bit_len });
    }

    Ok(bit_len.div_ceil(8) as usize)
}

fn normalize_bit_payload(bit_len: u64, mut payload: Vec<u8>) -> Vec<u8> {
    let remainder = bit_len % 8;
    if remainder != 0 {
        if let Some(last) = payload.last_mut() {
            *last &= 0xff << (8 - remainder);
        }
    }
    payload
}

fn accepts_mysql_bit(ty: &Type) -> bool {
    ty.oid() == Type::MYSQL_SYS_BIT.oid()
        && ty.name() == Type::MYSQL_SYS_BIT.name()
        && ty.schema() == Type::MYSQL_SYS_BIT.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_mysql_jsonpath(ty: &Type) -> bool {
    ty == &Type::JSONPATH
}

fn accepts_oracle_rowid(ty: &Type) -> bool {
    ty.oid() == Type::ORACLE_ROWID.oid()
        && ty.name() == Type::ORACLE_ROWID.name()
        && ty.schema() == Type::ORACLE_ROWID.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_oracle_yminterval(ty: &Type) -> bool {
    ty.oid() == Type::ORACLE_YMINTERVAL.oid()
        && ty.name() == Type::ORACLE_YMINTERVAL.name()
        && ty.schema() == Type::ORACLE_YMINTERVAL.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_oracle_dsinterval(ty: &Type) -> bool {
    ty.oid() == Type::ORACLE_DSINTERVAL.oid()
        && ty.name() == Type::ORACLE_DSINTERVAL.name()
        && ty.schema() == Type::ORACLE_DSINTERVAL.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_sqlserver_datetime2(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_DATETIME2
}

fn accepts_sqlserver_time(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_SYS_TIME
}

fn accepts_sqlserver_money(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_SYS_MONEY
}

fn accepts_sqlserver_rowversion(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_ROWVERSION
}

fn accepts_sqlserver_variant(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_SQL_VARIANT
}

fn accepts_sqlserver_tinyint(ty: &Type) -> bool {
    ty == &Type::SQLSERVER_TINYINT
}

fn decode_sqlserver_i64(raw: &[u8], type_name: &str) -> Result<i64, Box<dyn Error + Sync + Send>> {
    let bytes: [u8; 8] = raw
        .try_into()
        .map_err(|_| format!("invalid SQL Server {type_name} length: {}", raw.len()))?;
    Ok(i64::from_be_bytes(bytes))
}

fn encode_sqlserver_i64(value: i64, out: &mut BytesMut) {
    out.put_i64(value);
}

fn decode_interval(raw: &[u8]) -> Result<(i64, i32, i32), OracleIntervalError> {
    let raw: [u8; 16] = raw
        .try_into()
        .map_err(|_| OracleIntervalError::InvalidWireLength { actual: raw.len() })?;
    let time = i64::from_be_bytes(raw[..8].try_into().unwrap());
    let days = i32::from_be_bytes(raw[8..12].try_into().unwrap());
    let months = i32::from_be_bytes(raw[12..].try_into().unwrap());
    Ok((time, days, months))
}

fn encode_interval(time: i64, days: i32, months: i32, out: &mut BytesMut) {
    out.put_i64(time);
    out.put_i32(days);
    out.put_i32(months);
}

impl<'a> FromSql<'a> for OracleRowId {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<OracleRowId, Box<dyn Error + Sync + Send>> {
        Ok(OracleRowId(raw.to_vec()))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_rowid(ty)
    }
}

impl ToSql for OracleRowId {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_rowid(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlJsonPath {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlJsonPath, Box<dyn Error + Sync + Send>> {
        Ok(MySqlJsonPath::new(raw.to_vec()))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_jsonpath(ty)
    }
}

impl ToSql for MySqlJsonPath {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_jsonpath(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for OracleYmInterval {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<OracleYmInterval, Box<dyn Error + Sync + Send>> {
        let (nanoseconds, days, months) = decode_interval(raw)?;
        if nanoseconds != 0 || days != 0 {
            return Err(
                OracleIntervalError::YearMonthContainsDayOrTime { nanoseconds, days }.into(),
            );
        }
        Ok(OracleYmInterval::new(months))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_yminterval(ty)
    }
}

impl ToSql for OracleYmInterval {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_interval(0, 0, self.months, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_yminterval(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for OracleDsInterval {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<OracleDsInterval, Box<dyn Error + Sync + Send>> {
        let (nanoseconds, days, months) = decode_interval(raw)?;
        if months != 0 {
            return Err(OracleIntervalError::DaySecondContainsMonths { months }.into());
        }
        Ok(OracleDsInterval::new(days, nanoseconds))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_dsinterval(ty)
    }
}

impl ToSql for OracleDsInterval {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_interval(self.nanoseconds, self.days, 0, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_dsinterval(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for Interval {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Interval, Box<dyn Error + Sync + Send>> {
        let (microseconds, days, months) = decode_interval(raw)?;
        Ok(Interval::new(microseconds, days, months))
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::INTERVAL
    }
}

impl ToSql for Interval {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_interval(self.microseconds, self.days, self.months, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::INTERVAL
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerDateTime2 {
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> Result<SqlServerDateTime2, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerDateTime2::new(decode_sqlserver_i64(
            raw,
            "datetime2",
        )?))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_datetime2(ty)
    }
}

impl ToSql for SqlServerDateTime2 {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_sqlserver_i64(self.ticks, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_datetime2(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerTime {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerTime, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerTime::new(decode_sqlserver_i64(raw, "time")?)?)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_time(ty)
    }
}

impl ToSql for SqlServerTime {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_sqlserver_i64(self.ticks, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_time(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerMoney {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerMoney, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerMoney::new(decode_sqlserver_i64(raw, "money")?))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_money(ty)
    }
}

impl ToSql for SqlServerMoney {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_sqlserver_i64(self.scaled_value, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_money(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerRowVersion {
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> Result<SqlServerRowVersion, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 8] = raw
            .try_into()
            .map_err(|_| format!("invalid SQL Server rowversion length: {}", raw.len()))?;
        Ok(SqlServerRowVersion::new(bytes))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_rowversion(ty)
    }
}

impl ToSql for SqlServerRowVersion {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_rowversion(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerVariant {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerVariant, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerVariant::new(raw.to_vec()))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_variant(ty)
    }
}

impl ToSql for SqlServerVariant {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_variant(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerTinyInt {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerTinyInt, Box<dyn Error + Sync + Send>> {
        let value = raw
            .try_into()
            .map(u8::from_be_bytes)
            .map_err(|_| format!("invalid SQL Server tinyint length: {}", raw.len()))?;
        Ok(SqlServerTinyInt(value))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_tinyint(ty)
    }
}

impl ToSql for SqlServerTinyInt {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_u8(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_tinyint(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlTime {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlTime, Box<dyn Error + Sync + Send>> {
        MySqlTime::new(postgres_protocol::types::time_from_sql(raw)?).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::MYSQL_SYS_TIME
    }
}

impl ToSql for MySqlTime {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        postgres_protocol::types::time_to_sql(self.micros, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::MYSQL_SYS_TIME
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlBit {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlBit, Box<dyn Error + Sync + Send>> {
        if raw.len() < 8 {
            return Err(MySqlBitError::MissingLengthHeader { actual: raw.len() }.into());
        }

        // Kingbase MySQL BIT uses two uint32 metadata fields rather than the
        // PostgreSQL BIT/VARBIT wire format: leading zero bits followed by
        // the number of significant bits and their MSB-aligned payload.
        let leading_zero_bits = u32::from_be_bytes(raw[..4].try_into().unwrap()) as u64;
        let significant_bits = u32::from_be_bytes(raw[4..8].try_into().unwrap()) as u64;
        let bit_len = leading_zero_bits
            .checked_add(significant_bits)
            .ok_or(MySqlBitError::TooLong { bit_len: u64::MAX })?;
        // The payload is sized for the declared bit width (including an
        // all-zero value whose significant-bit count is zero).
        let expected = bit_payload_len(bit_len)?;
        if raw.len() != 8 + expected {
            return Err(MySqlBitError::InvalidPayloadLength {
                bit_len,
                expected,
                actual: raw.len() - 8,
            }
            .into());
        }

        let significant = &raw[8..];
        if (significant_bits as usize..significant.len() * 8)
            .any(|index| significant[index / 8] & (0x80 >> (index % 8)) != 0)
        {
            return Err("mysql bit payload has non-zero unused bits".into());
        }

        let total_len = bit_payload_len(bit_len)?;
        let mut payload = vec![0; total_len];
        for index in 0..significant_bits as usize {
            if significant[index / 8] & (0x80 >> (index % 8)) != 0 {
                let position = leading_zero_bits as usize + index;
                payload[position / 8] |= 0x80 >> (position % 8);
            }
        }

        MySqlBit::new(bit_len, payload).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_bit(ty)
    }
}

impl ToSql for MySqlBit {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let mut leading_zero_bits = self.bit_len;
        for index in 0..self.bit_len as usize {
            if self.payload[index / 8] & (0x80 >> (index % 8)) != 0 {
                leading_zero_bits = index as u64;
                break;
            }
        }
        // Kingbase accepts/returns the canonical parameter form for an
        // all-zero value as leading=0, significant=bit_len, zero payload.
        if leading_zero_bits == self.bit_len {
            leading_zero_bits = 0;
        }
        let significant_bits = self.bit_len - leading_zero_bits;
        out.put_u32(leading_zero_bits as u32);
        out.put_u32(significant_bits as u32);

        if significant_bits > 0 {
            let significant_len = bit_payload_len(significant_bits).expect("validated bit length");
            let mut significant = vec![0; significant_len];
            for index in 0..significant_bits as usize {
                let source = leading_zero_bits as usize + index;
                if self.payload[source / 8] & (0x80 >> (source % 8)) != 0 {
                    significant[index / 8] |= 0x80 >> (index % 8);
                }
            }
            out.extend_from_slice(&significant);
        }
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_bit(ty)
    }

    to_sql_checked!();
}
