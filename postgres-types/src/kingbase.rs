//! KingbaseES-specific Rust type wrappers.

use bytes::{BufMut, BytesMut};
use std::error::Error;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use crate::{FromSql, IsNull, Kind, ToSql, Type};

/// A KingbaseES SQL Server-compatible unsigned `sys.tinyint` value.
///
/// A wrapper is required because implementing `FromSql`/`ToSql` directly for
/// `u8` would overlap rust-postgres's generic `Vec<T>` array implementation
/// with its existing specialized `Vec<u8>` binary-value implementation.
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
/// The PostgreSQL protocol represents this value in a compact binary form,
/// while SQL displays it as a canonical 23-character string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OracleRowId(Vec<u8>);

/// An error converting an Oracle-compatible ROWID between binary and text forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleRowIdError {
    /// The binary value has an impossible size.
    InvalidBinaryLength {
        /// The received number of bytes.
        actual: usize,
    },
    /// The binary length header does not match the received value.
    InvalidLengthHeader {
        /// The number of following bytes declared by the header.
        declared: usize,
        /// The actual number of bytes following the header.
        actual: usize,
    },
    /// The field lengths encoded in the binary descriptor are invalid.
    InvalidFieldLengths,
    /// A textual ROWID is not exactly 23 ASCII characters.
    InvalidTextLength {
        /// The received UTF-8 byte length.
        actual: usize,
    },
    /// A textual ROWID contains a character outside its 64-character alphabet.
    InvalidCharacter {
        /// The zero-based byte position of the invalid character.
        index: usize,
        /// The invalid byte.
        byte: u8,
    },
    /// A textual field exceeds the range of its corresponding integer.
    FieldOutOfRange,
    /// Transaction ID zero is not a valid ROWID component.
    InvalidTransactionId,
}

impl fmt::Display for OracleRowIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            OracleRowIdError::InvalidBinaryLength { actual } => {
                write!(f, "invalid Oracle ROWID binary length: {actual}")
            }
            OracleRowIdError::InvalidLengthHeader { declared, actual } => write!(
                f,
                "invalid Oracle ROWID length header: declared {declared}, actual {actual}"
            ),
            OracleRowIdError::InvalidFieldLengths => {
                f.write_str("invalid Oracle ROWID field lengths")
            }
            OracleRowIdError::InvalidTextLength { actual } => {
                write!(f, "invalid Oracle ROWID text length: {actual}")
            }
            OracleRowIdError::InvalidCharacter { index, byte } => write!(
                f,
                "invalid Oracle ROWID character at index {index}: 0x{byte:02x}"
            ),
            OracleRowIdError::FieldOutOfRange => f.write_str("Oracle ROWID field is out of range"),
            OracleRowIdError::InvalidTransactionId => {
                f.write_str("Oracle ROWID transaction ID cannot be zero")
            }
        }
    }
}

impl Error for OracleRowIdError {}

impl OracleRowId {
    /// Creates an Oracle ROWID from its PostgreSQL-protocol binary payload
    /// without validating it.
    ///
    /// Prefer [`OracleRowId::try_from_bytes`] for values that do not come
    /// directly from a trusted KingbaseES server.
    pub fn new(bytes: impl Into<Vec<u8>>) -> OracleRowId {
        OracleRowId(bytes.into())
    }

    /// Creates an Oracle ROWID from a validated PostgreSQL-protocol binary payload.
    pub fn try_from_bytes(bytes: impl Into<Vec<u8>>) -> Result<OracleRowId, OracleRowIdError> {
        let bytes = bytes.into();
        decode_oracle_rowid_binary(&bytes)?;
        Ok(OracleRowId(bytes))
    }

    /// Parses the canonical 23-character Oracle ROWID representation.
    pub fn from_text(value: &str) -> Result<OracleRowId, OracleRowIdError> {
        let parts = decode_oracle_rowid_text(value)?;
        Ok(OracleRowId(encode_oracle_rowid_binary(parts)))
    }

    /// Returns the canonical 23-character Oracle ROWID representation.
    pub fn to_text(&self) -> Result<String, OracleRowIdError> {
        let parts = decode_oracle_rowid_binary(&self.0)?;
        Ok(encode_oracle_rowid_text(parts))
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

impl FromStr for OracleRowId {
    type Err = OracleRowIdError;

    fn from_str(value: &str) -> Result<OracleRowId, OracleRowIdError> {
        OracleRowId::from_text(value)
    }
}

impl fmt::Display for OracleRowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text().map_err(|_| fmt::Error)?)
    }
}

const ORACLE_ROWID_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct OracleRowIdParts {
    rollback: u32,
    transaction_id: u32,
    count: u64,
}

fn decode_oracle_rowid_binary(raw: &[u8]) -> Result<OracleRowIdParts, OracleRowIdError> {
    if !(4..=18).contains(&raw.len()) {
        return Err(OracleRowIdError::InvalidBinaryLength { actual: raw.len() });
    }

    let declared = usize::from(raw[0]);
    let actual = raw.len() - 1;
    if declared != actual {
        return Err(OracleRowIdError::InvalidLengthHeader { declared, actual });
    }

    let descriptor = raw[1];
    let rollback_len = usize::from(descriptor >> 5);
    let transaction_id_len = usize::from((descriptor >> 3) & 0x03) + 1;
    let count_len = usize::from(descriptor & 0x07) + 1;
    if rollback_len > 4 || 2 + rollback_len + transaction_id_len + count_len != raw.len() {
        return Err(OracleRowIdError::InvalidFieldLengths);
    }

    let mut offset = 2;
    let rollback = decode_oracle_rowid_binary_field(&raw[offset..offset + rollback_len]) as u32;
    offset += rollback_len;
    let transaction_id =
        decode_oracle_rowid_binary_field(&raw[offset..offset + transaction_id_len]) as u32;
    offset += transaction_id_len;
    let count = decode_oracle_rowid_binary_field(&raw[offset..offset + count_len]);

    if transaction_id == 0 {
        return Err(OracleRowIdError::InvalidTransactionId);
    }

    Ok(OracleRowIdParts {
        rollback,
        transaction_id,
        count,
    })
}

fn decode_oracle_rowid_binary_field(raw: &[u8]) -> u64 {
    raw.iter()
        .fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

fn decode_oracle_rowid_text(value: &str) -> Result<OracleRowIdParts, OracleRowIdError> {
    let raw = value.as_bytes();
    if raw.len() != 23 {
        return Err(OracleRowIdError::InvalidTextLength { actual: raw.len() });
    }

    let rollback = decode_oracle_rowid_text_field(&raw[..6], 0)?;
    let transaction_id = decode_oracle_rowid_text_field(&raw[6..12], 6)?;
    let count = decode_oracle_rowid_text_field(&raw[12..], 12)?;
    let rollback = u32::try_from(rollback).map_err(|_| OracleRowIdError::FieldOutOfRange)?;
    let transaction_id =
        u32::try_from(transaction_id).map_err(|_| OracleRowIdError::FieldOutOfRange)?;
    if transaction_id == 0 {
        return Err(OracleRowIdError::InvalidTransactionId);
    }

    Ok(OracleRowIdParts {
        rollback,
        transaction_id,
        count,
    })
}

fn decode_oracle_rowid_text_field(raw: &[u8], base_index: usize) -> Result<u64, OracleRowIdError> {
    raw.iter()
        .enumerate()
        .try_fold(0_u64, |value, (index, byte)| {
            let digit = match *byte {
                b'A'..=b'Z' => *byte - b'A',
                b'a'..=b'z' => *byte - b'a' + 26,
                b'0'..=b'9' => *byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => {
                    return Err(OracleRowIdError::InvalidCharacter {
                        index: base_index + index,
                        byte: *byte,
                    });
                }
            };
            value
                .checked_mul(64)
                .and_then(|value| value.checked_add(u64::from(digit)))
                .ok_or(OracleRowIdError::FieldOutOfRange)
        })
}

fn encode_oracle_rowid_binary(parts: OracleRowIdParts) -> Vec<u8> {
    let rollback = minimal_oracle_rowid_bytes(u64::from(parts.rollback), true);
    let transaction_id = minimal_oracle_rowid_bytes(u64::from(parts.transaction_id), false);
    let count = minimal_oracle_rowid_bytes(parts.count, false);
    let descriptor = ((rollback.len() as u8) << 5)
        | (((transaction_id.len() - 1) as u8) << 3)
        | ((count.len() - 1) as u8);
    let payload_len = 1 + rollback.len() + transaction_id.len() + count.len();

    let mut raw = Vec::with_capacity(payload_len + 1);
    raw.push(payload_len as u8);
    raw.push(descriptor);
    raw.extend_from_slice(&rollback);
    raw.extend_from_slice(&transaction_id);
    raw.extend_from_slice(&count);
    raw
}

fn minimal_oracle_rowid_bytes(value: u64, empty_zero: bool) -> Vec<u8> {
    if value == 0 {
        return if empty_zero { Vec::new() } else { vec![0] };
    }

    let bytes = value.to_be_bytes();
    let first = bytes.iter().position(|byte| *byte != 0).unwrap();
    bytes[first..].to_vec()
}

fn encode_oracle_rowid_text(parts: OracleRowIdParts) -> String {
    let mut raw = [b'A'; 23];
    encode_oracle_rowid_text_field(u64::from(parts.rollback), &mut raw[..6]);
    encode_oracle_rowid_text_field(u64::from(parts.transaction_id), &mut raw[6..12]);
    encode_oracle_rowid_text_field(parts.count, &mut raw[12..]);
    String::from_utf8(raw.to_vec()).expect("Oracle ROWID alphabet is ASCII")
}

fn encode_oracle_rowid_text_field(mut value: u64, output: &mut [u8]) {
    for byte in output.iter_mut().rev() {
        *byte = ORACLE_ROWID_ALPHABET[(value & 0x3f) as usize];
        value >>= 6;
    }
    debug_assert_eq!(value, 0);
}

/// A KingbaseES Oracle-compatible XMLType value.
///
/// Kingbase exposes XMLType as a PostgreSQL composite with one field named
/// `xml_data` whose type is XML. The wrapper therefore uses the same composite
/// binary layout as rust-postgres derives: field count, field OID, field
/// length, and field payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OracleXmlType {
    xml_data: String,
}

impl OracleXmlType {
    /// Creates an XMLType value from its XML text.
    pub fn new(xml_data: impl Into<String>) -> OracleXmlType {
        OracleXmlType {
            xml_data: xml_data.into(),
        }
    }

    /// Returns the XML text stored in this XMLType value.
    pub fn xml_data(&self) -> &str {
        &self.xml_data
    }

    /// Consumes this value and returns its XML text.
    pub fn into_xml_data(self) -> String {
        self.xml_data
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

    /// Creates a YEAR TO MONTH interval from separate Oracle components.
    ///
    /// The month component must be in `-11..=11`, and all components must
    /// have the same sign (or be zero), matching Oracle's interval rules.
    pub fn from_parts(years: i32, months: i32) -> Result<OracleYmInterval, OracleIntervalError> {
        if !(-11..=11).contains(&months) {
            return Err(OracleIntervalError::InvalidYearMonthComponents { years, months });
        }
        if (years > 0 && months < 0) || (years < 0 && months > 0) {
            return Err(OracleIntervalError::InvalidYearMonthComponents { years, months });
        }
        let total = years
            .checked_mul(12)
            .and_then(|value| value.checked_add(months))
            .ok_or(OracleIntervalError::InvalidYearMonthComponents { years, months })?;
        Ok(OracleYmInterval::new(total))
    }

    /// Returns the signed whole-year component.
    pub fn years(self) -> i32 {
        self.months / 12
    }

    /// Returns the signed remaining month component (`-11..=11`).
    pub fn months_part(self) -> i32 {
        self.months % 12
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

    /// Creates a DAY TO SECOND interval from Oracle-style components.
    ///
    /// Components must be uniformly non-negative or uniformly non-positive,
    /// and follow Oracle's day/hour/minute/second/nanosecond ranges.
    pub fn from_parts(
        days: i32,
        hours: i32,
        minutes: i32,
        seconds: i32,
        nanoseconds: i32,
    ) -> Result<OracleDsInterval, OracleIntervalError> {
        let valid_ranges = (-999_999_999..=999_999_999).contains(&days)
            && (-23..=23).contains(&hours)
            && (-59..=59).contains(&minutes)
            && (-59..=59).contains(&seconds)
            && (-999_999_999..=999_999_999).contains(&nanoseconds);
        let same_sign = [days, hours, minutes, seconds, nanoseconds]
            .iter()
            .all(|&value| value >= 0)
            || [days, hours, minutes, seconds, nanoseconds]
                .iter()
                .all(|&value| value <= 0);
        if !valid_ranges || !same_sign {
            return Err(OracleIntervalError::InvalidDaySecondComponents {
                days,
                hours,
                minutes,
                seconds,
                nanoseconds,
            });
        }

        let subday = i64::from(hours)
            .checked_mul(3_600_000_000_000)
            .and_then(|value| value.checked_add(i64::from(minutes) * 60_000_000_000))
            .and_then(|value| value.checked_add(i64::from(seconds) * 1_000_000_000))
            .and_then(|value| value.checked_add(i64::from(nanoseconds)))
            .ok_or(OracleIntervalError::InvalidDaySecondComponents {
                days,
                hours,
                minutes,
                seconds,
                nanoseconds,
            })?;
        Ok(OracleDsInterval::new(days, subday))
    }

    /// Returns the interval's whole-day component.
    pub fn days(self) -> i32 {
        self.days
    }

    /// Returns the interval's sub-day component in nanoseconds.
    pub fn nanoseconds(self) -> i64 {
        self.nanoseconds
    }

    /// Returns the signed hour component within the day.
    pub fn hours(self) -> i32 {
        (self.nanoseconds / 3_600_000_000_000) as i32
    }

    /// Returns the signed minute component within the hour.
    pub fn minutes(self) -> i32 {
        ((self.nanoseconds.abs() / 60_000_000_000) % 60) as i32 * self.nanoseconds.signum() as i32
    }

    /// Returns the signed second component within the minute.
    pub fn seconds(self) -> i32 {
        ((self.nanoseconds.abs() / 1_000_000_000) % 60) as i32 * self.nanoseconds.signum() as i32
    }

    /// Returns the signed nanosecond component within the second.
    pub fn subsecond_nanoseconds(self) -> i32 {
        (self.nanoseconds.abs() % 1_000_000_000) as i32 * self.nanoseconds.signum() as i32
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

/// A PostgreSQL-compatible `MONEY` value.
///
/// The wire representation stores the amount as an integer scaled by 100.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Money {
    scaled_value: i64,
}

impl Money {
    /// The number of stored units in one monetary unit.
    pub const SCALE: i64 = 100;

    /// Creates a MONEY value from its exact scaled integer representation.
    pub fn new(scaled_value: i64) -> Money {
        Money { scaled_value }
    }

    /// Returns the exact integer scaled by [`Money::SCALE`].
    pub fn scaled_value(self) -> i64 {
        self.scaled_value
    }
}

/// A PostgreSQL-compatible `TIMETZ` value.
///
/// The public offset uses the conventional seconds east of UTC representation,
/// while the PostgreSQL wire format stores seconds west of UTC.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TimeTz {
    microseconds: i64,
    utc_offset_seconds: i32,
}

impl TimeTz {
    /// The number of microseconds in one day.
    pub const MICROSECONDS_PER_DAY: i64 = 86_400_000_000;

    /// Creates a TIMETZ value from microseconds since midnight and seconds east of UTC.
    pub fn new(microseconds: i64, utc_offset_seconds: i32) -> Result<TimeTz, TimeTzError> {
        if !(0..Self::MICROSECONDS_PER_DAY).contains(&microseconds) {
            return Err(TimeTzError::TimeOutOfRange { microseconds });
        }
        if !(-57_599..=57_599).contains(&utc_offset_seconds) {
            return Err(TimeTzError::OffsetOutOfRange { utc_offset_seconds });
        }
        Ok(TimeTz {
            microseconds,
            utc_offset_seconds,
        })
    }

    /// Returns the number of microseconds since midnight.
    pub fn microseconds(self) -> i64 {
        self.microseconds
    }

    /// Returns the time-zone offset in seconds east of UTC.
    pub fn utc_offset_seconds(self) -> i32 {
        self.utc_offset_seconds
    }
}

/// An error returned when a TIMETZ component is outside its valid range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimeTzError {
    /// The time-of-day value is outside one day.
    TimeOutOfRange {
        /// The supplied number of microseconds since midnight.
        microseconds: i64,
    },
    /// The UTC offset exceeds PostgreSQL's maximum displacement.
    OffsetOutOfRange {
        /// The supplied number of seconds east of UTC.
        utc_offset_seconds: i32,
    },
}

impl fmt::Display for TimeTzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            TimeTzError::TimeOutOfRange { microseconds } => {
                write!(f, "invalid TIMETZ time-of-day microseconds: {microseconds}")
            }
            TimeTzError::OffsetOutOfRange { utc_offset_seconds } => {
                write!(f, "invalid TIMETZ UTC offset seconds: {utc_offset_seconds}")
            }
        }
    }
}

impl Error for TimeTzError {}

/// A PostgreSQL-compatible `REFCURSOR` portal name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefCursor(String);

impl RefCursor {
    /// Creates a REFCURSOR value from a portal name.
    pub fn new(name: impl Into<String>) -> RefCursor {
        RefCursor(name.into())
    }

    /// Returns the portal name.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns the portal name.
    pub fn into_name(self) -> String {
        self.0
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

    /// Creates a DATETIME2 value from signed 100-nanosecond ticks.
    pub fn new(ticks: i64) -> SqlServerDateTime2 {
        SqlServerDateTime2 { ticks }
    }

    /// Returns the signed 100-nanosecond ticks relative to `2000-01-01`.
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// A KingbaseES SQL Server-compatible `sys.time` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SqlServerTime {
    ticks: i64,
}

impl SqlServerTime {
    /// The number of TIME ticks in one second.
    pub const TICKS_PER_SECOND: i64 = 10_000_000;
    const TICKS_PER_DAY: i64 = 86_400 * Self::TICKS_PER_SECOND;

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
    /// Returns the invalid tick value.
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
    /// YEAR TO MONTH components violate Oracle's range or sign rules.
    InvalidYearMonthComponents {
        /// The supplied year component.
        years: i32,
        /// The supplied month component.
        months: i32,
    },
    /// DAY TO SECOND components violate Oracle's range or sign rules.
    InvalidDaySecondComponents {
        /// The supplied day component.
        days: i32,
        /// The supplied hour component.
        hours: i32,
        /// The supplied minute component.
        minutes: i32,
        /// The supplied second component.
        seconds: i32,
        /// The supplied nanosecond component.
        nanoseconds: i32,
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
            OracleIntervalError::InvalidYearMonthComponents { years, months } => write!(
                f,
                "invalid Oracle YEAR TO MONTH components: years={years}, months={months}"
            ),
            OracleIntervalError::InvalidDaySecondComponents {
                days,
                hours,
                minutes,
                seconds,
                nanoseconds,
            } => write!(
                f,
                "invalid Oracle DAY TO SECOND components: days={days}, hours={hours}, minutes={minutes}, seconds={seconds}, nanoseconds={nanoseconds}"
            ),
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

/// A MySQL `YEAR` value.
///
/// MySQL represents `YEAR` as either `0` or a four-digit year in the range
/// `1901..=2155`.  This wrapper keeps that semantic constraint separate from
/// the existing `i32` implementation, which remains available for backwards
/// compatibility with the PostgreSQL-style type table.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MySqlYear(i32);

/// An error returned when constructing a [`MySqlYear`] outside MySQL's range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MySqlYearError {
    year: i32,
}

impl MySqlYearError {
    /// Returns the rejected year.
    pub fn year(self) -> i32 {
        self.year
    }
}

impl fmt::Display for MySqlYearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid MySQL YEAR value: {}", self.year)
    }
}

impl Error for MySqlYearError {}

impl MySqlYear {
    /// Creates a MySQL YEAR value.
    ///
    /// `0` is accepted for MySQL compatibility.  Whether it can be stored is
    /// a server-mode policy; current Kingbase MySQL mode rejects zero years at
    /// the domain constraint layer.
    pub fn new(year: i32) -> Result<MySqlYear, MySqlYearError> {
        if year == 0 || (1901..=2155).contains(&year) {
            Ok(MySqlYear(year))
        } else {
            Err(MySqlYearError { year })
        }
    }

    /// Returns the numeric year.
    pub fn year(self) -> i32 {
        self.0
    }
}

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

    /// Creates a TIME value from MySQL-style components.
    ///
    /// The sign applies to the complete magnitude.  Minutes, seconds and
    /// microseconds are validated using the same ranges as mature MySQL
    /// drivers; the Kingbase wire representation is still the signed
    /// microsecond value handled by [`FromSql`] and [`ToSql`] below.
    pub fn from_parts(
        negative: bool,
        hours: u32,
        minutes: u8,
        seconds: u8,
        microseconds: u32,
    ) -> Result<MySqlTime, MySqlTimeError> {
        if minutes >= 60 || seconds >= 60 || microseconds >= 1_000_000 {
            return Err(MySqlTimeError { micros: i64::MAX });
        }

        let magnitude = i64::from(hours)
            .checked_mul(3_600_000_000)
            .and_then(|value| value.checked_add(i64::from(minutes) * 60_000_000))
            .and_then(|value| value.checked_add(i64::from(seconds) * 1_000_000))
            .and_then(|value| value.checked_add(i64::from(microseconds)))
            .ok_or(MySqlTimeError { micros: i64::MAX })?;
        if negative && magnitude == 0 {
            return Err(MySqlTimeError { micros: 0 });
        }
        let micros = if negative { -magnitude } else { magnitude };
        MySqlTime::new(micros)
    }

    /// Returns the signed duration in microseconds.
    pub fn micros(self) -> i64 {
        self.micros
    }

    /// Returns whether this TIME value is negative.
    pub fn is_negative(self) -> bool {
        self.micros < 0
    }

    /// Returns whether this TIME value is zero.
    pub fn is_zero(self) -> bool {
        self.micros == 0
    }

    /// Returns the absolute number of whole hours (0 through 838).
    pub fn hours(self) -> u32 {
        (self.micros.unsigned_abs() / 3_600_000_000) as u32
    }

    /// Returns the minute component (0 through 59).
    pub fn minutes(self) -> u8 {
        ((self.micros.unsigned_abs() / 60_000_000) % 60) as u8
    }

    /// Returns the second component (0 through 59).
    pub fn seconds(self) -> u8 {
        ((self.micros.unsigned_abs() / 1_000_000) % 60) as u8
    }

    /// Returns the microsecond component (0 through 999,999).
    pub fn microseconds(self) -> u32 {
        (self.micros.unsigned_abs() % 1_000_000) as u32
    }

    /// Returns whether this value can be represented as a non-negative time
    /// of day (`00:00:00` through `23:59:59.999999`).
    pub fn is_valid_time_of_day(self) -> bool {
        !self.is_negative() && self.hours() < 24
    }

    /// Converts a non-negative MySQL TIME value into a standard duration.
    /// Negative MySQL TIME values cannot be represented by
    /// [`std::time::Duration`].
    pub fn to_duration(self) -> Option<Duration> {
        if self.is_negative() {
            return None;
        }
        Some(Duration::from_micros(self.micros as u64))
    }

    /// Creates a positive MySQL TIME value from a standard duration.
    pub fn from_duration(duration: Duration) -> Result<MySqlTime, MySqlTimeError> {
        let micros = duration
            .as_secs()
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_add(u64::from(duration.subsec_micros())))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(MySqlTimeError { micros: i64::MAX })?;
        MySqlTime::new(micros)
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

    /// Returns the value as `u8`, rejecting values that do not fit.
    pub fn to_u8(&self) -> Result<u8, MySqlBitError> {
        u8::try_from(self.to_u64()).map_err(|_| MySqlBitError::ValueTooLarge {
            bit_len: 8,
            value: self.to_u64(),
        })
    }

    /// Returns the value as `u16`, rejecting values that do not fit.
    pub fn to_u16(&self) -> Result<u16, MySqlBitError> {
        u16::try_from(self.to_u64()).map_err(|_| MySqlBitError::ValueTooLarge {
            bit_len: 16,
            value: self.to_u64(),
        })
    }

    /// Returns the value as `u32`, rejecting values that do not fit.
    pub fn to_u32(&self) -> Result<u32, MySqlBitError> {
        u32::try_from(self.to_u64()).map_err(|_| MySqlBitError::ValueTooLarge {
            bit_len: 32,
            value: self.to_u64(),
        })
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

fn accepts_oracle_xmltype(ty: &Type) -> bool {
    if ty.name() != "xmltype" || ty.schema() != "public" {
        return false;
    }

    match ty.kind() {
        Kind::Composite(fields) if fields.len() == 1 => {
            fields[0].name() == "xml_data" && fields[0].type_() == &Type::XML
        }
        _ => false,
    }
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

fn decode_i64(raw: &[u8], type_name: &str) -> Result<i64, Box<dyn Error + Sync + Send>> {
    let bytes: [u8; 8] = raw
        .try_into()
        .map_err(|_| format!("invalid {type_name} length: {}", raw.len()))?;
    Ok(i64::from_be_bytes(bytes))
}

fn encode_i64(value: i64, out: &mut BytesMut) {
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
        OracleRowId::try_from_bytes(raw.to_vec()).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_rowid(ty)
    }
}

impl ToSql for OracleRowId {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        decode_oracle_rowid_binary(&self.0)?;
        out.extend_from_slice(&self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_rowid(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for OracleXmlType {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<OracleXmlType, Box<dyn Error + Sync + Send>> {
        if !accepts_oracle_xmltype(ty) {
            return Err("invalid XMLType metadata".into());
        }

        let fields = match ty.kind() {
            Kind::Composite(fields) => fields,
            _ => unreachable!(),
        };
        let mut buf = raw;
        let count = crate::private::read_be_i32(&mut buf)?;
        if count != 1 {
            return Err(format!("invalid XMLType field count: {count}").into());
        }

        let oid = crate::private::read_be_i32(&mut buf)? as u32;
        if oid != fields[0].type_().oid() {
            return Err("unexpected XMLType field OID".into());
        }

        let xml_data: String = crate::private::read_value(fields[0].type_(), &mut buf)?;
        if !buf.is_empty() {
            return Err("trailing bytes in XMLType value".into());
        }
        Ok(OracleXmlType::new(xml_data))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_xmltype(ty)
    }
}

impl ToSql for OracleXmlType {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        if !accepts_oracle_xmltype(ty) {
            return Err("invalid XMLType metadata".into());
        }

        let fields = match ty.kind() {
            Kind::Composite(fields) => fields,
            _ => unreachable!(),
        };
        out.extend_from_slice(&1_i32.to_be_bytes());
        out.extend_from_slice(&fields[0].type_().oid().to_be_bytes());
        let length_offset = out.len();
        out.extend_from_slice(&[0; 4]);
        let payload_start = out.len();
        match self.xml_data.to_sql(fields[0].type_(), out)? {
            IsNull::Yes => {
                out[length_offset..length_offset + 4].copy_from_slice(&(-1_i32).to_be_bytes());
            }
            IsNull::No => {
                let len = out.len() - payload_start;
                let len = i32::try_from(len).map_err(|_| "XMLType value too large to transmit")?;
                out[length_offset..length_offset + 4].copy_from_slice(&len.to_be_bytes());
            }
        }
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_oracle_xmltype(ty)
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

/*
impl<'a> FromSql<'a> for MySqlYear {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlYear, Box<dyn Error + Sync + Send>> {
        let value = postgres_protocol::types::int4_from_sql(raw)?;
        MySqlYear::new(value).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::MYSQL_YEAR | Type::INT4)
    }
}

impl ToSql for MySqlYear {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        postgres_protocol::types::int4_to_sql(self.0, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::MYSQL_YEAR | Type::INT4)
    }

    to_sql_checked!();
}
*/

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

impl<'a> FromSql<'a> for Money {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Money, Box<dyn Error + Sync + Send>> {
        Ok(Money::new(decode_i64(raw, "MONEY")?))
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::MONEY
    }
}

impl ToSql for Money {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_i64(self.scaled_value, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::MONEY
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for TimeTz {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<TimeTz, Box<dyn Error + Sync + Send>> {
        let raw: [u8; 12] = raw
            .try_into()
            .map_err(|_| format!("invalid TIMETZ length: {}", raw.len()))?;
        let microseconds = i64::from_be_bytes(raw[..8].try_into().unwrap());
        let seconds_west = i32::from_be_bytes(raw[8..].try_into().unwrap());
        let utc_offset_seconds = seconds_west
            .checked_neg()
            .ok_or("invalid TIMETZ UTC offset")?;
        TimeTz::new(microseconds, utc_offset_seconds).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::TIMETZ
    }
}

impl ToSql for TimeTz {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_i64(self.microseconds);
        out.put_i32(-self.utc_offset_seconds);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::TIMETZ
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for RefCursor {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<RefCursor, Box<dyn Error + Sync + Send>> {
        Ok(RefCursor::new(std::str::from_utf8(raw)?))
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::REFCURSOR
    }
}

impl ToSql for RefCursor {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.extend_from_slice(self.0.as_bytes());
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty == &Type::REFCURSOR
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerDateTime2 {
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> Result<SqlServerDateTime2, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerDateTime2::new(decode_i64(
            raw,
            "SQL Server datetime2",
        )?))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_datetime2(ty)
    }
}

impl ToSql for SqlServerDateTime2 {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_i64(self.ticks, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_datetime2(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerTime {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerTime, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerTime::new(decode_i64(raw, "SQL Server time")?)?)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_time(ty)
    }
}

impl ToSql for SqlServerTime {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_i64(self.ticks, out);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_time(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for SqlServerMoney {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SqlServerMoney, Box<dyn Error + Sync + Send>> {
        Ok(SqlServerMoney::new(decode_i64(raw, "SQL Server money")?))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_sqlserver_money(ty)
    }
}

impl ToSql for SqlServerMoney {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        encode_i64(self.scaled_value, out);
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
        let bytes: [u8; 1] = raw
            .try_into()
            .map_err(|_| format!("invalid SQL Server tinyint length: {}", raw.len()))?;
        Ok(SqlServerTinyInt(u8::from_be_bytes(bytes)))
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
