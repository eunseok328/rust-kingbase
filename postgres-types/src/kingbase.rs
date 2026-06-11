//! Kingbase-specific Rust type wrappers.

use bytes::{BufMut, BytesMut};
use std::error::Error;
use std::fmt;

use crate::{FromSql, IsNull, Kind, ToSql, Type};

/// A Kingbase MySQL-compatible `sys.tinyint` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TinyInt(pub i8);

/// A Kingbase MySQL-compatible `sys.uint4` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MySqlUInt4(pub u32);

/// A Kingbase MySQL-compatible `sys.uint8` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MySqlUInt8(pub u64);

/// An error returned when a [`Year`] value is outside Kingbase's supported range.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct YearOutOfRange {
    value: i32,
}

impl YearOutOfRange {
    /// Returns the rejected year value.
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

/// A Kingbase MySQL-compatible `sys.year` value.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Year(i32);

impl Year {
    /// The minimum supported `sys.year` value.
    pub const MIN: i32 = 1901;

    /// The maximum supported `sys.year` value.
    pub const MAX: i32 = 2155;

    /// Creates a `Year`, validating the Kingbase MySQL-compatible range.
    pub fn new(value: i32) -> Result<Year, YearOutOfRange> {
        if (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Year(value))
        } else {
            Err(YearOutOfRange { value })
        }
    }

    /// Returns the year as an integer.
    pub fn get(self) -> i32 {
        self.0
    }
}

/// An error returned when constructing or decoding a [`MySqlBit`] value fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MySqlBitError {
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
    /// The binary value is shorter than the 8-byte length header.
    MissingLengthHeader {
        /// The actual wire payload length in bytes.
        actual: usize,
    },
}

impl fmt::Display for MySqlBitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
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

/// A Kingbase MySQL-compatible `sys.bit` value.
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

fn accepts_tinyint(ty: &Type) -> bool {
    ty.oid() == Type::MYSQL_TINYINT.oid()
        && ty.name() == Type::MYSQL_TINYINT.name()
        && ty.schema() == Type::MYSQL_TINYINT.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_year(ty: &Type) -> bool {
    if ty.oid() != Type::MYSQL_YEAR.oid()
        || ty.name() != Type::MYSQL_YEAR.name()
        || ty.schema() != Type::MYSQL_YEAR.schema()
    {
        return false;
    }

    matches!(
        ty.kind(),
        Kind::Domain(base)
            if base.is_equivalent_to(&Type::MYSQL_INT4) || base.is_equivalent_to(&Type::INT4)
    )
}

fn accepts_mysql_bit(ty: &Type) -> bool {
    ty.oid() == Type::MYSQL_SYS_BIT.oid()
        && ty.name() == Type::MYSQL_SYS_BIT.name()
        && ty.schema() == Type::MYSQL_SYS_BIT.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_mysql_uint4(ty: &Type) -> bool {
    ty.oid() == Type::MYSQL_UINT4.oid()
        && ty.name() == Type::MYSQL_UINT4.name()
        && ty.schema() == Type::MYSQL_UINT4.schema()
        && matches!(ty.kind(), Kind::Simple)
}

fn accepts_mysql_uint8(ty: &Type) -> bool {
    ty.oid() == Type::MYSQL_UINT8.oid()
        && ty.name() == Type::MYSQL_UINT8.name()
        && ty.schema() == Type::MYSQL_UINT8.schema()
        && matches!(ty.kind(), Kind::Simple)
}

impl<'a> FromSql<'a> for TinyInt {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<TinyInt, Box<dyn Error + Sync + Send>> {
        if raw.len() != 1 {
            return Err(format!("invalid tinyint length: {}", raw.len()).into());
        }
        Ok(TinyInt(raw[0] as i8))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_tinyint(ty)
    }
}

impl ToSql for TinyInt {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_i8(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_tinyint(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlUInt4 {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlUInt4, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 4] = raw
            .try_into()
            .map_err(|_| format!("invalid mysql uint4 length: {}", raw.len()))?;
        Ok(MySqlUInt4(u32::from_be_bytes(bytes)))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_uint4(ty)
    }
}

impl ToSql for MySqlUInt4 {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_u32(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_uint4(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlUInt8 {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlUInt8, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 8] = raw
            .try_into()
            .map_err(|_| format!("invalid mysql uint8 length: {}", raw.len()))?;
        Ok(MySqlUInt8(u64::from_be_bytes(bytes)))
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_uint8(ty)
    }
}

impl ToSql for MySqlUInt8 {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_u64(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_uint8(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for Year {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Year, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 4] = raw
            .try_into()
            .map_err(|_| format!("invalid year length: {}", raw.len()))?;
        Year::new(i32::from_be_bytes(bytes)).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_year(ty)
    }
}

impl ToSql for Year {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_i32(self.0);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_year(ty)
    }

    to_sql_checked!();
}

impl<'a> FromSql<'a> for MySqlBit {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<MySqlBit, Box<dyn Error + Sync + Send>> {
        if raw.len() < 8 {
            return Err(MySqlBitError::MissingLengthHeader { actual: raw.len() }.into());
        }

        let bit_len = u64::from_be_bytes(raw[..8].try_into().unwrap());
        let expected = bit_payload_len(bit_len)?;
        let payload =
            raw[8..]
                .get(..expected)
                .ok_or_else(|| MySqlBitError::InvalidPayloadLength {
                    bit_len,
                    expected,
                    actual: raw.len() - 8,
                })?;

        MySqlBit::new(bit_len, payload.to_vec()).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_bit(ty)
    }
}

impl ToSql for MySqlBit {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        out.put_u64(self.bit_len);
        out.extend_from_slice(&self.payload);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        accepts_mysql_bit(ty)
    }

    to_sql_checked!();
}

#[cfg(test)]
mod tests {
    use super::{MySqlBit, MySqlBitError, MySqlUInt4, MySqlUInt8, TinyInt, Year};
    use crate::{FromSql, IsNull, Kind, ToSql, Type};
    use bytes::BytesMut;

    #[test]
    fn tinyint_accepts_only_sys_tinyint() {
        assert!(<TinyInt as FromSql>::accepts(&Type::MYSQL_TINYINT));
        assert!(<TinyInt as ToSql>::accepts(&Type::MYSQL_TINYINT));
        assert!(!<TinyInt as FromSql>::accepts(&Type::CHAR));
        assert!(!<TinyInt as ToSql>::accepts(&Type::MYSQL_INT1));
    }

    #[test]
    fn year_validates_range_and_accepts_only_sys_year() {
        assert_eq!(Year::new(1901).unwrap().get(), 1901);
        assert_eq!(Year::new(2155).unwrap().get(), 2155);
        assert_eq!(Year::new(1900).unwrap_err().value(), 1900);
        assert_eq!(Year::new(2156).unwrap_err().value(), 2156);
        assert_eq!(Year::new(24).unwrap_err().value(), 24);

        assert!(<Year as ToSql>::accepts(&Type::MYSQL_YEAR));
        assert!(<Year as FromSql>::accepts(&Type::MYSQL_YEAR));
        assert!(!<Year as ToSql>::accepts(&Type::INT4));
        assert!(!<Year as FromSql>::accepts(&Type::MYSQL_SYS_JSON));
    }

    #[test]
    fn mysql_bit_normalizes_payload_and_accepts_only_sys_bit() {
        let false_bit = MySqlBit::from_bool(false);
        assert_eq!(false_bit.bit_len(), 1);
        assert_eq!(false_bit.payload(), &[0x00]);
        assert_eq!(false_bit.as_bool(), Some(false));

        let true_bit = MySqlBit::from_bool(true);
        assert_eq!(true_bit.bit_len(), 1);
        assert_eq!(true_bit.payload(), &[0x80]);
        assert_eq!(true_bit.as_bool(), Some(true));

        let normalized = MySqlBit::new(9, vec![0xaa, 0xff]).unwrap();
        assert_eq!(normalized.bit_len(), 9);
        assert_eq!(normalized.payload(), &[0xaa, 0x80]);
        assert_eq!(normalized.as_bool(), None);

        assert_eq!(
            MySqlBit::new(65, vec![0; 9]).unwrap_err(),
            MySqlBitError::TooLong { bit_len: 65 }
        );
        assert!(matches!(
            MySqlBit::new(8, Vec::<u8>::new()).unwrap_err(),
            MySqlBitError::InvalidPayloadLength { .. }
        ));
        assert!(MySqlBit::new(0, Vec::<u8>::new()).is_ok());

        assert!(<MySqlBit as ToSql>::accepts(&Type::MYSQL_SYS_BIT));
        assert!(<MySqlBit as FromSql>::accepts(&Type::MYSQL_SYS_BIT));
        assert!(!<MySqlBit as ToSql>::accepts(&Type::BIT));
        assert!(!<MySqlBit as FromSql>::accepts(&Type::VARBIT));
    }

    #[test]
    fn mysql_uint4_accepts_only_exact_sys_uint4() {
        let wrong_kind = Type::new(
            Type::MYSQL_UINT4.name().to_string(),
            Type::MYSQL_UINT4.oid(),
            Kind::Domain(Type::MYSQL_INT4),
            Type::MYSQL_UINT4.schema().to_string(),
        );

        assert!(<MySqlUInt4 as ToSql>::accepts(&Type::MYSQL_UINT4));
        assert!(<MySqlUInt4 as FromSql>::accepts(&Type::MYSQL_UINT4));
        assert!(!<MySqlUInt4 as ToSql>::accepts(&Type::OID));
        assert!(!<MySqlUInt4 as FromSql>::accepts(&Type::OID));
        assert!(!<MySqlUInt4 as ToSql>::accepts(&Type::INT4));
        assert!(!<MySqlUInt4 as FromSql>::accepts(&Type::INT4));
        assert!(!<MySqlUInt4 as ToSql>::accepts(&Type::MYSQL_INT4));
        assert!(!<MySqlUInt4 as FromSql>::accepts(&Type::MYSQL_INT4));
        assert!(!<MySqlUInt4 as ToSql>::accepts(&wrong_kind));
        assert!(!<MySqlUInt4 as FromSql>::accepts(&wrong_kind));

        assert!(<Vec<MySqlUInt4> as ToSql>::accepts(
            &Type::MYSQL_UINT4_ARRAY
        ));
        assert!(<Vec<MySqlUInt4> as FromSql<'_>>::accepts(
            &Type::MYSQL_UINT4_ARRAY
        ));
    }

    #[test]
    fn mysql_uint4_uses_big_endian_unsigned_payload() {
        assert_eq!(
            <MySqlUInt4 as FromSql>::from_sql(&Type::MYSQL_UINT4, &[0x89, 0xab, 0xcd, 0xef])
                .unwrap(),
            MySqlUInt4(0x89abcdef)
        );
        assert!(<MySqlUInt4 as FromSql>::from_sql(&Type::MYSQL_UINT4, &[0]).is_err());

        let mut out = BytesMut::new();
        match MySqlUInt4(0x89abcdef)
            .to_sql(&Type::MYSQL_UINT4, &mut out)
            .unwrap()
        {
            IsNull::No => {}
            IsNull::Yes => panic!("expected non-null mysql uint4 encoding"),
        }
        assert_eq!(&out[..], &[0x89, 0xab, 0xcd, 0xef]);
    }

    #[test]
    fn mysql_uint8_accepts_only_exact_sys_uint8() {
        let wrong_kind = Type::new(
            Type::MYSQL_UINT8.name().to_string(),
            Type::MYSQL_UINT8.oid(),
            Kind::Domain(Type::MYSQL_INT8),
            Type::MYSQL_UINT8.schema().to_string(),
        );

        assert!(<MySqlUInt8 as ToSql>::accepts(&Type::MYSQL_UINT8));
        assert!(<MySqlUInt8 as FromSql>::accepts(&Type::MYSQL_UINT8));
        assert!(!<MySqlUInt8 as ToSql>::accepts(&Type::OID));
        assert!(!<MySqlUInt8 as FromSql>::accepts(&Type::OID));
        assert!(!<MySqlUInt8 as ToSql>::accepts(&Type::INT8));
        assert!(!<MySqlUInt8 as FromSql>::accepts(&Type::INT8));
        assert!(!<MySqlUInt8 as ToSql>::accepts(&Type::MYSQL_INT8));
        assert!(!<MySqlUInt8 as FromSql>::accepts(&Type::MYSQL_INT8));
        assert!(!<MySqlUInt8 as ToSql>::accepts(&wrong_kind));
        assert!(!<MySqlUInt8 as FromSql>::accepts(&wrong_kind));

        assert!(<Vec<MySqlUInt8> as ToSql>::accepts(
            &Type::MYSQL_UINT8_ARRAY
        ));
        assert!(<Vec<MySqlUInt8> as FromSql<'_>>::accepts(
            &Type::MYSQL_UINT8_ARRAY
        ));
    }

    #[test]
    fn mysql_uint8_uses_big_endian_unsigned_payload() {
        assert_eq!(
            <MySqlUInt8 as FromSql>::from_sql(
                &Type::MYSQL_UINT8,
                &[0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76],
            )
            .unwrap(),
            MySqlUInt8(0x89abcdef10325476)
        );
        assert!(<MySqlUInt8 as FromSql>::from_sql(&Type::MYSQL_UINT8, &[0]).is_err());

        let mut out = BytesMut::new();
        match MySqlUInt8(0x89abcdef10325476)
            .to_sql(&Type::MYSQL_UINT8, &mut out)
            .unwrap()
        {
            IsNull::No => {}
            IsNull::Yes => panic!("expected non-null mysql uint8 encoding"),
        }
        assert_eq!(&out[..], &[0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76]);
    }
}
