//! Codec behavior tests kept outside the postgres-types implementation crate.

use postgres_types::kingbase::{
    MySqlBit, MySqlBitError, MySqlJsonPath, OracleDsInterval,
    OracleIntervalError, OracleRowId, OracleYmInterval, SqlServerDateTime2,
    SqlServerMoney, SqlServerRowVersion, SqlServerTime, SqlServerTinyInt, SqlServerVariant,
};
use postgres_types::{FromSql, IsNull, Kind, ToSql, Type};
use bytes::BytesMut;

fn numeric(weight: i16, sign: u16, dscale: i16, digits: &[i16]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(8 + digits.len() * 2);
    raw.extend_from_slice(&(digits.len() as i16).to_be_bytes());
    raw.extend_from_slice(&weight.to_be_bytes());
    raw.extend_from_slice(&sign.to_be_bytes());
    raw.extend_from_slice(&dscale.to_be_bytes());
    for digit in digits {
        raw.extend_from_slice(&digit.to_be_bytes());
    }
    raw
}

#[test]
fn mysql_numeric_i64_decodes_integral_values_exactly() {
    assert_eq!(
        <i64 as FromSql>::from_sql(&Type::MYSQL_NUMERIC, &numeric(0, 0x0000, 2, &[42])).unwrap(),
        42
    );
    assert_eq!(
        <i64 as FromSql>::from_sql(
            &Type::MYSQL_NUMERIC,
            &numeric(4, 0x0000, 0, &[922, 3372, 368, 5477, 5807]),
        )
        .unwrap(),
        i64::MAX
    );
    assert_eq!(
        <i64 as FromSql>::from_sql(
            &Type::MYSQL_NUMERIC,
            &numeric(4, 0x4000, 0, &[922, 3372, 368, 5477, 5808]),
        )
        .unwrap(),
        i64::MIN
    );
}

#[test]
fn mysql_numeric_i64_rejects_fractional_and_invalid_values() {
    let fractional = numeric(0, 0x0000, 2, &[1, 2500]);
    assert!(<i64 as FromSql>::from_sql(&Type::MYSQL_NUMERIC, &fractional).is_err());

    let below_one = numeric(-1, 0x4000, 4, &[12]);
    assert!(<i64 as FromSql>::from_sql(&Type::MYSQL_NUMERIC, &below_one).is_err());

    let overflow = numeric(4, 0x0000, 0, &[922, 3372, 368, 5477, 5808]);
    assert!(<i64 as FromSql>::from_sql(&Type::MYSQL_NUMERIC, &overflow).is_err());

    let nan = numeric(0, 0xC000, 0, &[]);
    assert!(<i64 as FromSql>::from_sql(&Type::MYSQL_NUMERIC, &nan).is_err());
}

#[test]
fn i8_accepts_mysql_tinyint() {
    assert!(<i8 as FromSql>::accepts(&Type::MYSQL_TINYINT));
    assert!(<i8 as ToSql>::accepts(&Type::MYSQL_TINYINT));
    assert!(<i8 as FromSql>::accepts(&Type::CHAR));
    assert!(!<i8 as ToSql>::accepts(&Type::MYSQL_INT1));
}

#[test]
fn sqlserver_tinyint_round_trips_unsigned_byte() {
    let value = SqlServerTinyInt::from_sql(&Type::SQLSERVER_TINYINT, &[0xff]).unwrap();
    assert_eq!(value, SqlServerTinyInt(255));
    assert!(<SqlServerTinyInt as FromSql>::accepts(&Type::SQLSERVER_TINYINT));
    assert!(<SqlServerTinyInt as ToSql>::accepts(&Type::SQLSERVER_TINYINT));
    assert!(!<SqlServerTinyInt as FromSql>::accepts(&Type::MYSQL_TINYINT));
    assert!(!<SqlServerTinyInt as ToSql>::accepts(&Type::ORACLE_TINYINT));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_TINYINT, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), &[0xff]);
    assert!(SqlServerTinyInt::from_sql(&Type::SQLSERVER_TINYINT, &[]).is_err());
    assert!(SqlServerTinyInt::from_sql(&Type::SQLSERVER_TINYINT, &[0, 1]).is_err());
}

#[test]
fn oracle_rowid_round_trips_binary_payload() {
    let payload = [0x04, 0x08, 0xe0, 0x04, 0x00];
    let rowid = <OracleRowId as FromSql>::from_sql(&Type::ORACLE_ROWID, &payload).unwrap();

    assert_eq!(rowid.as_bytes(), payload);
    assert!(<OracleRowId as FromSql>::accepts(&Type::ORACLE_ROWID));
    assert!(<OracleRowId as ToSql>::accepts(&Type::ORACLE_ROWID));
    assert!(!<OracleRowId as FromSql>::accepts(&Type::MYSQL_ROWID));
    assert!(!<OracleRowId as ToSql>::accepts(&Type::TEXT));

    let mut encoded = BytesMut::new();
    assert!(matches!(
        rowid.to_sql(&Type::ORACLE_ROWID, &mut encoded).unwrap(),
        IsNull::No
    ));
    assert_eq!(encoded.as_ref(), payload);
    assert_eq!(rowid.clone().into_bytes(), payload);
}

#[test]
fn kingbase_xml_round_trips_utf8_payload() {
    let payload = b"<root><answer>42</answer></root>";
    let xml = <String as FromSql>::from_sql(&Type::ORACLE_XML, payload).unwrap();

    assert_eq!(xml, "<root><answer>42</answer></root>");
    assert!(<String as FromSql>::accepts(&Type::MYSQL_XML));
    assert!(<String as ToSql>::accepts(&Type::ORACLE_XML));
    assert!(<String as FromSql>::accepts(&Type::SQLSERVER_XML));

    let mut encoded = BytesMut::new();
    assert!(matches!(
        xml.to_sql(&Type::MYSQL_XML, &mut encoded).unwrap(),
        IsNull::No
    ));
    assert_eq!(encoded.as_ref(), payload);
    assert!(<String as FromSql>::from_sql(&Type::MYSQL_XML, &[0xff]).is_err());
}

#[test]
fn mysql_jsonpath_round_trips_server_binary_payload() {
    let payload = [1, b'$', b'.', b'"', b'n', b'a', b'm', b'e', b'"'];
    let value = <MySqlJsonPath as FromSql>::from_sql(&Type::MYSQL_JSONPATH, &payload).unwrap();

    assert_eq!(value.as_bytes(), payload);
    assert!(<MySqlJsonPath as FromSql>::accepts(&Type::MYSQL_JSONPATH));
    assert!(<MySqlJsonPath as ToSql>::accepts(&Type::MYSQL_JSONPATH));
    assert!(!<MySqlJsonPath as ToSql>::accepts(
        &Type::MYSQL_JSONPATH_ARRAY
    ));
    assert!(<Vec<MySqlJsonPath> as FromSql>::accepts(
        &Type::MYSQL_JSONPATH_ARRAY
    ));
    assert!(<Vec<MySqlJsonPath> as ToSql>::accepts(
        &Type::MYSQL_JSONPATH_ARRAY
    ));

    let mut encoded = BytesMut::new();
    assert!(matches!(
        value.to_sql(&Type::MYSQL_JSONPATH, &mut encoded).unwrap(),
        IsNull::No
    ));
    assert_eq!(encoded.as_ref(), payload);
    assert_eq!(value.into_bytes(), payload);
}

#[test]
fn mysql_dynamic_enum_and_set_round_trip_text_payloads() {
    let enum_type = Type::new(
        "Enum_1".to_owned(),
        100_001,
        Kind::MySqlEnum(vec!["draft".to_owned(), "published".to_owned()]),
        "public".to_owned(),
    );
    let set_type = Type::new(
        "Set_1".to_owned(),
        100_002,
        Kind::MySqlSet,
        "public".to_owned(),
    );

    assert!(<String as FromSql>::accepts(&enum_type));
    assert!(<&str as ToSql>::accepts(&enum_type));
    assert_eq!(<String as FromSql>::from_sql(&enum_type, b"draft").unwrap(), "draft");

    assert!(<String as FromSql>::accepts(&set_type));
    assert!(<&str as ToSql>::accepts(&set_type));
    assert_eq!(<String as FromSql>::from_sql(&set_type, b"red,blue").unwrap(), "red,blue");

    let mut encoded = BytesMut::new();
    "published".to_sql(&enum_type, &mut encoded).unwrap();
    assert_eq!(encoded.as_ref(), b"published");

    encoded.clear();
    "red".to_sql(&set_type, &mut encoded).unwrap();
    assert_eq!(encoded.as_ref(), b"red");
}

#[test]
fn oracle_intervals_round_trip_server_wire_payloads() {
    let ym_payload = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 14];
    let ym =
        <OracleYmInterval as FromSql>::from_sql(&Type::ORACLE_YMINTERVAL, &ym_payload).unwrap();
    assert_eq!(ym.months(), 14);
    assert!(<OracleYmInterval as FromSql>::accepts(
        &Type::ORACLE_YMINTERVAL
    ));
    assert!(<OracleYmInterval as ToSql>::accepts(
        &Type::ORACLE_YMINTERVAL
    ));
    assert!(!<OracleYmInterval as ToSql>::accepts(
        &Type::ORACLE_DSINTERVAL
    ));

    let mut ym_encoded = BytesMut::new();
    ym.to_sql(&Type::ORACLE_YMINTERVAL, &mut ym_encoded)
        .unwrap();
    assert_eq!(ym_encoded.as_ref(), ym_payload);

    let ds_payload = [0, 0, 6, 0xb7, 0x56, 0x7f, 0xd5, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    let ds =
        <OracleDsInterval as FromSql>::from_sql(&Type::ORACLE_DSINTERVAL, &ds_payload).unwrap();
    assert_eq!(ds.days(), 1);
    assert_eq!(ds.nanoseconds(), 7_384_500_000_000);
    assert!(<OracleDsInterval as FromSql>::accepts(
        &Type::ORACLE_DSINTERVAL
    ));
    assert!(<OracleDsInterval as ToSql>::accepts(
        &Type::ORACLE_DSINTERVAL
    ));
    assert!(!<OracleDsInterval as ToSql>::accepts(
        &Type::ORACLE_YMINTERVAL
    ));

    let mut ds_encoded = BytesMut::new();
    ds.to_sql(&Type::ORACLE_DSINTERVAL, &mut ds_encoded)
        .unwrap();
    assert_eq!(ds_encoded.as_ref(), ds_payload);
}

#[test]
fn oracle_intervals_reject_incompatible_components() {
    let ym_error = <OracleYmInterval as FromSql>::from_sql(
        &Type::ORACLE_YMINTERVAL,
        &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0],
    )
    .unwrap_err();
    assert!(matches!(
        ym_error.downcast_ref::<OracleIntervalError>(),
        Some(OracleIntervalError::YearMonthContainsDayOrTime {
            nanoseconds: 1,
            days: 1,
        })
    ));

    let ds_error = <OracleDsInterval as FromSql>::from_sql(
        &Type::ORACLE_DSINTERVAL,
        &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    )
    .unwrap_err();
    assert!(matches!(
        ds_error.downcast_ref::<OracleIntervalError>(),
        Some(OracleIntervalError::DaySecondContainsMonths { months: 1 })
    ));
}

#[test]
fn sqlserver_datetime2_round_trips_100_nanosecond_ticks() {
    let payload = [0x00, 0x1a, 0xe9, 0x3d, 0x32, 0xee, 0x57, 0x07];
    let value = <SqlServerDateTime2 as FromSql>::from_sql(&Type::SQLSERVER_DATETIME2, &payload)
        .unwrap();

    assert_eq!(value.ticks(), 7_574_798_451_234_567);
    assert_eq!(SqlServerDateTime2::TICKS_PER_SECOND, 10_000_000);
    assert!(<SqlServerDateTime2 as FromSql>::accepts(
        &Type::SQLSERVER_DATETIME2
    ));
    assert!(<SqlServerDateTime2 as ToSql>::accepts(
        &Type::SQLSERVER_DATETIME2
    ));
    assert!(!<SqlServerDateTime2 as FromSql>::accepts(&Type::TIMESTAMP));
    assert!(!<SqlServerDateTime2 as ToSql>::accepts(
        &Type::SQLSERVER_DATETIME
    ));
    assert!(<Vec<SqlServerDateTime2> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_DATETIME2_ARRAY
    ));
    assert!(<Vec<SqlServerDateTime2> as ToSql>::accepts(
        &Type::SQLSERVER_DATETIME2_ARRAY
    ));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_DATETIME2, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), payload);
    assert!(
        <SqlServerDateTime2 as FromSql>::from_sql(&Type::SQLSERVER_DATETIME2, &[0; 7]).is_err()
    );
}

#[test]
fn sqlserver_time_round_trips_100_nanosecond_ticks() {
    let payload = [0x00, 0x00, 0x00, 0x69, 0x76, 0x97, 0xee, 0x87];
    let value =
        <SqlServerTime as FromSql>::from_sql(&Type::SQLSERVER_SYS_TIME, &payload).unwrap();

    assert_eq!(value.ticks(), 452_961_234_567);
    assert_eq!(SqlServerTime::TICKS_PER_SECOND, 10_000_000);
    assert!(<SqlServerTime as FromSql>::accepts(
        &Type::SQLSERVER_SYS_TIME
    ));
    assert!(<SqlServerTime as ToSql>::accepts(&Type::SQLSERVER_SYS_TIME));
    assert!(!<SqlServerTime as FromSql>::accepts(&Type::TIME));
    assert!(!<SqlServerTime as ToSql>::accepts(
        &Type::SQLSERVER_DATETIME2
    ));
    assert!(<Vec<SqlServerTime> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_SYS_TIME_ARRAY
    ));
    assert!(<Vec<SqlServerTime> as ToSql>::accepts(
        &Type::SQLSERVER_SYS_TIME_ARRAY
    ));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_SYS_TIME, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), payload);

    assert!(SqlServerTime::new(-1).is_err());
    assert!(SqlServerTime::new(24 * 60 * 60 * SqlServerTime::TICKS_PER_SECOND).is_err());
    assert!(<SqlServerTime as FromSql>::from_sql(&Type::SQLSERVER_SYS_TIME, &[0; 7]).is_err());
    assert!(
        <SqlServerTime as FromSql>::from_sql(
            &Type::SQLSERVER_SYS_TIME,
            &(24 * 60 * 60 * SqlServerTime::TICKS_PER_SECOND).to_be_bytes(),
        )
        .is_err()
    );
}

#[test]
fn sqlserver_money_round_trips_scaled_integer() {
    let payload = [0x00, 0x00, 0x00, 0x00, 0x00, 0x12, 0xd6, 0x87];
    let value =
        <SqlServerMoney as FromSql>::from_sql(&Type::SQLSERVER_SYS_MONEY, &payload).unwrap();

    assert_eq!(value.scaled_value(), 1_234_567);
    assert_eq!(SqlServerMoney::SCALE, 10_000);
    assert!(<SqlServerMoney as FromSql>::accepts(
        &Type::SQLSERVER_SYS_MONEY
    ));
    assert!(<SqlServerMoney as ToSql>::accepts(
        &Type::SQLSERVER_SYS_MONEY
    ));
    assert!(!<SqlServerMoney as FromSql>::accepts(&Type::MONEY));
    assert!(!<SqlServerMoney as ToSql>::accepts(&Type::XID8));
    assert!(<Vec<SqlServerMoney> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_SYS_MONEY_ARRAY
    ));
    assert!(<Vec<SqlServerMoney> as ToSql>::accepts(
        &Type::SQLSERVER_SYS_MONEY_ARRAY
    ));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_SYS_MONEY, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), payload);
    assert!(
        <SqlServerMoney as FromSql>::from_sql(&Type::SQLSERVER_SYS_MONEY, &[0; 7]).is_err()
    );
}

#[test]
fn sqlserver_rowversion_round_trips_exact_bytes() {
    let payload = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02];
    let value =
        <SqlServerRowVersion as FromSql>::from_sql(&Type::SQLSERVER_ROWVERSION, &payload)
            .unwrap();

    assert_eq!(value.as_bytes(), &payload);
    assert!(<SqlServerRowVersion as FromSql>::accepts(
        &Type::SQLSERVER_ROWVERSION
    ));
    assert!(<SqlServerRowVersion as ToSql>::accepts(
        &Type::SQLSERVER_ROWVERSION
    ));
    assert!(!<SqlServerRowVersion as FromSql>::accepts(&Type::BYTEA));
    assert!(!<SqlServerRowVersion as ToSql>::accepts(
        &Type::SQLSERVER_SYS_TIME
    ));
    assert!(<Vec<SqlServerRowVersion> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_ROWVERSION_ARRAY
    ));
    assert!(<Vec<SqlServerRowVersion> as ToSql>::accepts(
        &Type::SQLSERVER_ROWVERSION_ARRAY
    ));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_ROWVERSION, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), payload);
    assert!(
        <SqlServerRowVersion as FromSql>::from_sql(&Type::SQLSERVER_ROWVERSION, &[0; 7])
            .is_err()
    );
}

#[test]
fn sqlserver_variant_round_trips_server_binary_payload() {
    let payload = [
        0x01, 0x10, 0x0c, 0x00, 0xc3, 0x0f, 0x0c, 0x00, b'v', b'a', b'r', b'i', b'a', b'n',
        b't', b'-', b't', b'e', b'x', b't',
    ];
    let value = <SqlServerVariant as FromSql>::from_sql(&Type::SQLSERVER_SQL_VARIANT, &payload)
        .unwrap();

    assert_eq!(value.as_bytes(), payload);
    assert!(<SqlServerVariant as FromSql>::accepts(
        &Type::SQLSERVER_SQL_VARIANT
    ));
    assert!(<SqlServerVariant as ToSql>::accepts(
        &Type::SQLSERVER_SQL_VARIANT
    ));
    assert!(!<SqlServerVariant as FromSql>::accepts(&Type::TEXT));
    assert!(!<SqlServerVariant as ToSql>::accepts(&Type::BYTEA));
    assert!(!<SqlServerVariant as ToSql>::accepts(
        &Type::SQLSERVER_SQL_VARIANT_ARRAY
    ));

    let mut encoded = BytesMut::new();
    value
        .to_sql(&Type::SQLSERVER_SQL_VARIANT, &mut encoded)
        .unwrap();
    assert_eq!(encoded.as_ref(), payload);
    assert_eq!(value.into_bytes(), payload);
}

#[test]
fn i32_accepts_sys_year() {
    assert!(<i32 as ToSql>::accepts(&Type::MYSQL_YEAR));
    assert!(<i32 as FromSql>::accepts(&Type::MYSQL_YEAR));

    let mut encoded = BytesMut::new();
    2026_i32.to_sql(&Type::MYSQL_YEAR, &mut encoded).unwrap();
    assert_eq!(encoded.as_ref(), &2026_i32.to_be_bytes());
    assert_eq!(<i32 as FromSql>::from_sql(&Type::MYSQL_YEAR, &2026_i32.to_be_bytes()).unwrap(), 2026);
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
fn u32_accepts_only_exact_mysql_uint4() {
    let wrong_kind = Type::new(
        Type::MYSQL_UINT4.name().to_string(),
        Type::MYSQL_UINT4.oid(),
        Kind::Domain(Type::MYSQL_INT4),
        Type::MYSQL_UINT4.schema().to_string(),
    );

    assert!(<u32 as ToSql>::accepts(&Type::MYSQL_UINT4));
    assert!(<u32 as FromSql>::accepts(&Type::MYSQL_UINT4));
    assert!(<u32 as ToSql>::accepts(&Type::OID));
    assert!(<u32 as FromSql>::accepts(&Type::OID));
    assert!(!<u32 as ToSql>::accepts(&Type::INT4));
    assert!(!<u32 as FromSql>::accepts(&Type::INT4));
    assert!(!<u32 as ToSql>::accepts(&Type::MYSQL_INT4));
    assert!(!<u32 as FromSql>::accepts(&Type::MYSQL_INT4));
    assert!(!<u32 as ToSql>::accepts(&wrong_kind));
    assert!(!<u32 as FromSql>::accepts(&wrong_kind));

    assert!(<Vec<u32> as ToSql>::accepts(&Type::MYSQL_UINT4_ARRAY));
    assert!(<Vec<u32> as FromSql<'_>>::accepts(
        &Type::MYSQL_UINT4_ARRAY
    ));
}

#[test]
fn u32_uses_mysql_uint4_big_endian_unsigned_payload() {
    assert_eq!(
        <u32 as FromSql>::from_sql(&Type::MYSQL_UINT4, &[0x89, 0xab, 0xcd, 0xef]).unwrap(),
        0x89abcdef
    );
    assert!(<u32 as FromSql>::from_sql(&Type::MYSQL_UINT4, &[0]).is_err());

    let mut out = BytesMut::new();
    match 0x89abcdef_u32.to_sql(&Type::MYSQL_UINT4, &mut out).unwrap() {
        IsNull::No => {}
        IsNull::Yes => panic!("expected non-null mysql uint4 encoding"),
    }
    assert_eq!(&out[..], &[0x89, 0xab, 0xcd, 0xef]);
}

#[test]
fn u64_accepts_only_exact_mysql_uint8() {
    let wrong_kind = Type::new(
        Type::MYSQL_UINT8.name().to_string(),
        Type::MYSQL_UINT8.oid(),
        Kind::Domain(Type::MYSQL_INT8),
        Type::MYSQL_UINT8.schema().to_string(),
    );

    assert!(<u64 as ToSql>::accepts(&Type::MYSQL_UINT8));
    assert!(<u64 as FromSql>::accepts(&Type::MYSQL_UINT8));
    assert!(!<u64 as ToSql>::accepts(&Type::OID));
    assert!(!<u64 as FromSql>::accepts(&Type::OID));
    assert!(!<u64 as ToSql>::accepts(&Type::INT8));
    assert!(!<u64 as FromSql>::accepts(&Type::INT8));
    assert!(!<u64 as ToSql>::accepts(&Type::MYSQL_INT8));
    assert!(!<u64 as FromSql>::accepts(&Type::MYSQL_INT8));
    assert!(!<u64 as ToSql>::accepts(&wrong_kind));
    assert!(!<u64 as FromSql>::accepts(&wrong_kind));

    assert!(<Vec<u64> as ToSql>::accepts(&Type::MYSQL_UINT8_ARRAY));
    assert!(<Vec<u64> as FromSql<'_>>::accepts(
        &Type::MYSQL_UINT8_ARRAY
    ));
}

#[test]
fn u64_uses_mysql_uint8_big_endian_unsigned_payload() {
    assert_eq!(
        <u64 as FromSql>::from_sql(
            &Type::MYSQL_UINT8,
            &[0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76],
        )
        .unwrap(),
        0x89abcdef10325476
    );
    assert!(<u64 as FromSql>::from_sql(&Type::MYSQL_UINT8, &[0]).is_err());

    let mut out = BytesMut::new();
    match 0x89abcdef10325476_u64
        .to_sql(&Type::MYSQL_UINT8, &mut out)
        .unwrap()
    {
        IsNull::No => {}
        IsNull::Yes => panic!("expected non-null mysql uint8 encoding"),
    }
    assert_eq!(&out[..], &[0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76]);
}
