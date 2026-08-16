use std::time::SystemTime;

use tokio_postgres::types::kingbase::{MySqlBit, SqlServerTinyInt};
use tokio_postgres::types::{FromSql, Kind, PgLsn, ToSql, Type, TypeSystem};

#[test]
fn generated_type_modes_dispatch_by_schema() {
    let int4 = Type::from_oid(23).unwrap();
    assert_eq!(int4, Type::INT4);
    assert_eq!(int4.name(), "int4");
    assert_eq!(int4.schema(), "pg_catalog");

    let mysql_binary = Type::from_kingbase_mysql_oid(3383).unwrap();
    assert_eq!(mysql_binary.name(), "binary");
    assert_eq!(mysql_binary.schema(), "sys");

    assert!(Type::from_kingbase_mysql_oid(23).is_none());
    assert_eq!(Type::from_oid_in(TypeSystem::Mysql, 23), Some(Type::INT4));
    assert_eq!(Type::from_oid_in(TypeSystem::Mysql, 1082), Some(Type::DATE));

    let mysql_sys_date = Type::from_kingbase_mysql_oid(7944).unwrap();
    assert_eq!(mysql_sys_date.name(), "date");
    assert_eq!(mysql_sys_date.schema(), "sys");

    let sqlserver_nvarchar = Type::from_kingbase_sqlserver_oid(5041).unwrap();
    assert_eq!(sqlserver_nvarchar.name(), "nvarchar");
    assert_eq!(sqlserver_nvarchar.schema(), "sys");

    assert!(Type::from_kingbase_sqlserver_oid(23).is_none());
    assert_eq!(
        Type::from_oid_in(TypeSystem::SqlServer, 23),
        Some(Type::INT4)
    );
}

#[test]
fn compatibility_domains_reuse_only_explicit_rust_conversions() {
    for type_ in [
        &Type::MYSQL_INT3,
        &Type::MYSQL_MEDIUMINT,
        &Type::MYSQL_MIDDLEINT,
        &Type::MYSQL_YEAR,
    ] {
        assert!(<i32 as FromSql<'_>>::accepts(type_), "{type_:?}");
        assert!(<i32 as ToSql>::accepts(type_), "{type_:?}");
    }

    for type_ in [
        &Type::MYSQL_LONGTEXT,
        &Type::MYSQL_MEDIUMTEXT,
        &Type::MYSQL_TINYTEXT,
        &Type::MYSQL_CLOB,
        &Type::MYSQL_NCLOB,
        &Type::ORACLE_UROWID,
        &Type::ORACLE_CLOB,
        &Type::ORACLE_NCLOB,
        &Type::SQLSERVER_SYSNAME,
    ] {
        assert!(<&str as FromSql<'_>>::accepts(type_), "{type_:?}");
        assert!(<&str as ToSql>::accepts(type_), "{type_:?}");
    }

    for type_ in [
        &Type::MYSQL_BLOB,
        &Type::MYSQL_LONGBLOB,
        &Type::MYSQL_MEDIUMBLOB,
        &Type::MYSQL_TINYBLOB,
        &Type::ORACLE_BLOB,
    ] {
        assert!(<Vec<u8> as FromSql<'_>>::accepts(type_), "{type_:?}");
        assert!(<Vec<u8> as ToSql>::accepts(type_), "{type_:?}");
    }

    assert!(<SystemTime as FromSql<'_>>::accepts(&Type::ORACLE_SYS_DATE));
    assert!(<SystemTime as ToSql>::accepts(&Type::ORACLE_SYS_DATE));
    assert!(<PgLsn as FromSql<'_>>::accepts(&Type::SQLSERVER_SYS_LSN));
    assert!(<PgLsn as ToSql>::accepts(&Type::SQLSERVER_SYS_LSN));
    #[cfg(feature = "with-serde_json-1")]
    {
        assert!(<serde_json_1::Value as FromSql<'_>>::accepts(
            &Type::MYSQL_SYS_JSON
        ));
        assert!(<serde_json_1::Value as ToSql>::accepts(
            &Type::MYSQL_SYS_JSON
        ));
    }

    let custom_int_domain = Type::new(
        "account_id".to_string(),
        90_001,
        Kind::Domain(Type::INT4),
        "public".to_string(),
    );
    let custom_text_domain = Type::new(
        "label".to_string(),
        90_002,
        Kind::Domain(Type::TEXT),
        "public".to_string(),
    );
    assert!(!<i32 as FromSql<'_>>::accepts(&custom_int_domain));
    assert!(!<i32 as ToSql>::accepts(&custom_int_domain));
    assert!(!<&str as FromSql<'_>>::accepts(&custom_text_domain));
    assert!(!<&str as ToSql>::accepts(&custom_text_domain));
}

#[test]
fn sqlserver_mode_includes_observed_fixed_type_registrations() {
    for oid in [16, 17, 20, 21, 23, 25, 705, 1043, 1114, 1700] {
        let type_ = Type::from_oid_in(TypeSystem::SqlServer, oid)
            .unwrap_or_else(|| panic!("missing shared PostgreSQL type OID {oid}"));
        assert_eq!(type_.type_system(), Some(TypeSystem::Pg));
    }

    for oid in [
        4193, 5022, 5023, 5024, 5025, 5026, 5027, 5039, 5040, 5041, 5042, 5043, 5044, 5045,
        5046, 5069, 5070, 6080, 6661, 6662, 7141, 7680, 7690, 7754, 7764, 7755, 7765, 7881,
        7891, 7915, 7916, 8016, 8017, 8018, 8019, 12464, 12465, 12476,
    ] {
        let type_ = Type::from_oid_in(TypeSystem::SqlServer, oid)
            .unwrap_or_else(|| panic!("missing SQL Server type OID {oid}"));
        assert_eq!(type_.type_system(), Some(TypeSystem::SqlServer));
    }

    assert!(Type::from_oid_in(TypeSystem::SqlServer, 0).is_none());

    assert_eq!(
        Type::SQLSERVER_SYSNAME.kind(),
        &Kind::Domain(Type::SQLSERVER_NVARCHAR)
    );
    assert_eq!(Type::SQLSERVER_SYS_LSN.kind(), &Kind::Domain(Type::PG_LSN));
}

#[test]
fn sqlserver_text_types_reuse_text_codec_and_keep_other_special_types_strict() {
    assert!(<&str as FromSql<'_>>::accepts(&Type::SQLSERVER_NVARCHAR));
    assert!(<&str as ToSql>::accepts(&Type::SQLSERVER_NVARCHAR));
    assert!(<String as FromSql<'_>>::accepts(&Type::SQLSERVER_NVARCHAR));
    assert!(<String as ToSql>::accepts(&Type::SQLSERVER_NVARCHAR));
    assert!(<&str as FromSql<'_>>::accepts(&Type::SQLSERVER_NCHAR));
    assert!(<&str as ToSql>::accepts(&Type::SQLSERVER_NCHAR));
    assert!(<String as FromSql<'_>>::accepts(&Type::SQLSERVER_NCHAR));
    assert!(<String as ToSql>::accepts(&Type::SQLSERVER_NCHAR));
    assert!(<&str as FromSql<'_>>::accepts(&Type::SQLSERVER_SYSNAME));
    assert!(<&str as ToSql>::accepts(&Type::SQLSERVER_SYSNAME));
    assert!(<String as FromSql<'_>>::accepts(&Type::SQLSERVER_SYSNAME));
    assert!(<String as ToSql>::accepts(&Type::SQLSERVER_SYSNAME));
    assert!(<Vec<String> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_NVARCHAR_ARRAY
    ));
    assert!(<Vec<String> as ToSql>::accepts(
        &Type::SQLSERVER_NVARCHAR_ARRAY
    ));
    assert!(<Vec<String> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_NCHAR_ARRAY
    ));
    assert!(<Vec<String> as ToSql>::accepts(
        &Type::SQLSERVER_NCHAR_ARRAY
    ));
    assert!(<&str as FromSql<'_>>::accepts(&Type::SQLSERVER_BPCHARBYTE));
    assert!(<&str as ToSql>::accepts(&Type::SQLSERVER_VARCHARBYTE));

    assert!(!<&str as FromSql<'_>>::accepts(&Type::SQLSERVER_SYS_BIT));
    assert!(!<&str as ToSql>::accepts(&Type::SQLSERVER_DATETIME2));
}

#[test]
fn sqlserver_jdbc_fixed_binary_and_tinyint_types_keep_verified_boundaries() {
    assert!(<Vec<u8> as FromSql<'_>>::accepts(&Type::SQLSERVER_BINARY));
    assert!(<Vec<u8> as ToSql>::accepts(&Type::SQLSERVER_BINARY));
    assert!(<Vec<u8> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_VARBINARY
    ));
    assert!(<Vec<u8> as ToSql>::accepts(&Type::SQLSERVER_VARBINARY));
    assert!(<Vec<Vec<u8>> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_BINARY_ARRAY
    ));
    assert!(<Vec<Vec<u8>> as ToSql>::accepts(
        &Type::SQLSERVER_VARBINARY_ARRAY
    ));

    assert!(<SqlServerTinyInt as FromSql<'_>>::accepts(
        &Type::SQLSERVER_TINYINT
    ));
    assert!(<SqlServerTinyInt as ToSql>::accepts(
        &Type::SQLSERVER_TINYINT
    ));
    assert!(<Vec<SqlServerTinyInt> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_TINYINT_ARRAY
    ));
    assert!(<Vec<SqlServerTinyInt> as ToSql>::accepts(
        &Type::SQLSERVER_TINYINT_ARRAY
    ));

    assert!(!<i8 as ToSql>::accepts(&Type::SQLSERVER_TINYINT));
    assert!(!<SqlServerTinyInt as ToSql>::accepts(&Type::MYSQL_TINYINT));
    assert!(!<Vec<u8> as ToSql>::accepts(&Type::SQLSERVER_ROWVERSION));
}

#[test]
fn sqlserver_bit_reuses_bool_codec_with_exact_type_identity() {
    assert!(<bool as FromSql<'_>>::accepts(&Type::SQLSERVER_SYS_BIT));
    assert!(<bool as ToSql>::accepts(&Type::SQLSERVER_SYS_BIT));
    assert!(<Vec<bool> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_SYS_BIT_ARRAY
    ));
    assert!(<Vec<bool> as ToSql>::accepts(
        &Type::SQLSERVER_SYS_BIT_ARRAY
    ));

    assert!(!<bool as FromSql<'_>>::accepts(&Type::BIT));
    assert!(!<bool as ToSql>::accepts(&Type::MYSQL_SYS_BIT));
}

#[test]
fn sqlserver_datetime_types_reuse_pg_timestamp_codecs() {
    assert!(<SystemTime as FromSql<'_>>::accepts(
        &Type::SQLSERVER_DATETIME
    ));
    assert!(<SystemTime as ToSql>::accepts(&Type::SQLSERVER_DATETIME));
    assert!(<Vec<SystemTime> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_DATETIME_ARRAY
    ));
    assert!(<Vec<SystemTime> as ToSql>::accepts(
        &Type::SQLSERVER_DATETIME_ARRAY
    ));

    assert!(<SystemTime as FromSql<'_>>::accepts(
        &Type::SQLSERVER_SMALLDATETIME
    ));
    assert!(<SystemTime as ToSql>::accepts(
        &Type::SQLSERVER_SMALLDATETIME
    ));
    assert!(<Vec<SystemTime> as FromSql<'_>>::accepts(
        &Type::SQLSERVER_SMALLDATETIME_ARRAY
    ));
    assert!(<Vec<SystemTime> as ToSql>::accepts(
        &Type::SQLSERVER_SMALLDATETIME_ARRAY
    ));

    assert!(!<SystemTime as FromSql<'_>>::accepts(
        &Type::SQLSERVER_DATETIME2
    ));
    assert!(!<SystemTime as ToSql>::accepts(&Type::SQLSERVER_DATETIME2));
}

#[test]
fn sqlserver_sys_lsn_reuses_pg_lsn_codec_through_domain() {
    assert_eq!(Type::SQLSERVER_SYS_LSN.kind(), &Kind::Domain(Type::PG_LSN));
    assert!(<PgLsn as FromSql<'_>>::accepts(&Type::SQLSERVER_SYS_LSN));
    assert!(<PgLsn as ToSql>::accepts(&Type::SQLSERVER_SYS_LSN));
    assert!(<PgLsn as FromSql<'_>>::accepts(&Type::PG_LSN));
    assert!(<PgLsn as ToSql>::accepts(&Type::PG_LSN));
}

#[test]
fn oracle_mode_includes_jdbc_fixed_type_registrations() {
    for oid in [
        16, 17, 18, 19, 20, 21, 23, 25, 26, 27, 114, 142, 143, 199, 600, 700, 701, 790, 791, 1000,
        1001, 1002, 1003, 1005, 1007, 1009, 1010, 1014, 1015, 1016, 1017, 1021, 1022, 1028, 1042,
        1043, 1082, 1083, 1114, 1115, 1182, 1183, 1184, 1185, 1231, 1266, 1270, 1560, 1561, 1700,
        1790, 2201, 2950, 2951, 3802, 3807, 6123, 6124, 7000, 7001, 7002, 7003, 8013, 8014, 8015,
        8016, 8017, 8018, 8019, 8020, 8021, 8100, 8101,
    ] {
        let type_ = Type::from_oid_in(TypeSystem::Oracle, oid)
            .unwrap_or_else(|| panic!("missing Oracle type OID {oid}"));
        let expected_system = if Type::from_kingbase_oracle_oid(oid).is_some() {
            Some(TypeSystem::Oracle)
        } else {
            Some(TypeSystem::Pg)
        };
        assert_eq!(type_.type_system(), expected_system);
    }

    assert!(Type::from_oid_in(TypeSystem::Oracle, 0).is_none());
}

#[test]
fn basic_rust_types_accept_mysql_mode_builtins() {
    assert!(<bool as ToSql>::accepts(&Type::from_mysql_oid(16).unwrap()));
    assert!(<i8 as ToSql>::accepts(&Type::MYSQL_TINYINT));
    assert!(<i8 as FromSql<'_>>::accepts(&Type::MYSQL_TINYINT));
    assert!(<i16 as ToSql>::accepts(&Type::from_mysql_oid(21).unwrap()));
    assert!(<i32 as ToSql>::accepts(&Type::from_mysql_oid(23).unwrap()));
    assert!(<i32 as ToSql>::accepts(&Type::MYSQL_INT3));
    assert!(<i32 as ToSql>::accepts(&Type::MYSQL_MIDDLEINT));
    assert!(<u32 as ToSql>::accepts(&Type::MYSQL_UINT4));
    assert!(<u32 as FromSql<'_>>::accepts(&Type::MYSQL_UINT4));
    assert!(<u64 as ToSql>::accepts(&Type::MYSQL_UINT8));
    assert!(<u64 as FromSql<'_>>::accepts(&Type::MYSQL_UINT8));
    assert!(<i64 as ToSql>::accepts(&Type::from_mysql_oid(20).unwrap()));
    assert!(<f32 as ToSql>::accepts(&Type::from_mysql_oid(700).unwrap()));
    assert!(<f64 as ToSql>::accepts(&Type::from_mysql_oid(701).unwrap()));

    assert!(<String as FromSql<'_>>::accepts(
        &Type::from_oid_in(TypeSystem::Mysql, 25).unwrap()
    ));
    assert!(<&str as ToSql>::accepts(
        &Type::from_oid_in(TypeSystem::Mysql, 1043).unwrap()
    ));
    assert!(<&str as FromSql<'_>>::accepts(&Type::MYSQL_LONGTEXT));
    assert!(<&str as FromSql<'_>>::accepts(&Type::MYSQL_CLOB));
    assert!(<&str as ToSql>::accepts(&Type::MYSQL_NCLOB));
    assert!(<&str as FromSql<'_>>::accepts(&Type::MYSQL_BPCHARBYTE));
    assert!(<&str as ToSql>::accepts(&Type::MYSQL_VARCHARBYTE));
    assert!(<Vec<String> as FromSql<'_>>::accepts(
        &Type::MYSQL_BPCHARBYTE_ARRAY
    ));
    assert!(<Vec<String> as ToSql>::accepts(
        &Type::MYSQL_VARCHARBYTE_ARRAY
    ));
}

#[test]
fn basic_rust_types_accept_oracle_mode_shared_builtins() {
    assert!(<bool as ToSql>::accepts(&Type::BOOL));
    assert!(<i16 as ToSql>::accepts(&Type::INT2));
    assert!(<i32 as ToSql>::accepts(&Type::INT4));
    assert!(<i64 as ToSql>::accepts(&Type::INT8));
    assert!(<f32 as ToSql>::accepts(&Type::FLOAT4));
    assert!(<f64 as ToSql>::accepts(&Type::FLOAT8));
    assert!(<&str as ToSql>::accepts(&Type::VARCHAR));
    assert!(<&str as FromSql<'_>>::accepts(&Type::ORACLE_CLOB));
    assert!(<&[u8] as ToSql>::accepts(&Type::ORACLE_BLOB));
    assert!(<Vec<u8> as FromSql<'_>>::accepts(&Type::BYTEA));
    assert!(<Vec<i32> as ToSql>::accepts(&Type::INT4_ARRAY));
    assert!(<Vec<String> as FromSql<'_>>::accepts(&Type::VARCHAR_ARRAY));
}

#[test]
fn mysql_mode_arrays_use_mysql_member_types() {
    let mysql_int4_array = Type::from_oid_in(TypeSystem::Mysql, 1007).unwrap();
    assert_eq!(mysql_int4_array, Type::INT4_ARRAY);
    match mysql_int4_array.kind() {
        Kind::Array(member) => assert_eq!(member, &Type::INT4),
        kind => panic!("expected array type, got {kind:?}"),
    }
    assert!(<Vec<i32> as FromSql<'_>>::accepts(&mysql_int4_array));
    assert!(<Vec<i32> as ToSql>::accepts(&mysql_int4_array));

    let mysql_binary_array = Type::from_mysql_oid(3385).unwrap();
    assert_eq!(mysql_binary_array, Type::MYSQL_BINARY_ARRAY);
    match mysql_binary_array.kind() {
        Kind::Array(member) => assert_eq!(member, &Type::MYSQL_BINARY),
        kind => panic!("expected array type, got {kind:?}"),
    }
    assert!(<Vec<Vec<u8>> as FromSql<'_>>::accepts(&mysql_binary_array));
    assert!(<Vec<Vec<u8>> as ToSql>::accepts(&mysql_binary_array));

    let mysql_bit_array = Type::from_mysql_oid(4656).unwrap();
    assert_eq!(mysql_bit_array, Type::MYSQL_SYS_BIT_ARRAY);
    match mysql_bit_array.kind() {
        Kind::Array(member) => assert_eq!(member, &Type::MYSQL_SYS_BIT),
        kind => panic!("expected array type, got {kind:?}"),
    }
    assert!(<Vec<MySqlBit> as FromSql<'_>>::accepts(&mysql_bit_array));
    assert!(<Vec<MySqlBit> as ToSql>::accepts(&mysql_bit_array));

    let mysql_tinyint_array = Type::from_mysql_oid(8101).unwrap();
    assert_eq!(mysql_tinyint_array, Type::MYSQL_TINYINT_ARRAY);
    assert!(<Vec<i8> as FromSql<'_>>::accepts(&mysql_tinyint_array));
    assert!(<Vec<i8> as ToSql>::accepts(&mysql_tinyint_array));

    let mysql_uint4_array = Type::from_mysql_oid(7083).unwrap();
    assert_eq!(mysql_uint4_array, Type::MYSQL_UINT4_ARRAY);
    assert!(<Vec<u32> as FromSql<'_>>::accepts(&mysql_uint4_array));
    assert!(<Vec<u32> as ToSql>::accepts(&mysql_uint4_array));

    let mysql_uint8_array = Type::from_mysql_oid(7085).unwrap();
    assert_eq!(mysql_uint8_array, Type::MYSQL_UINT8_ARRAY);
    assert!(<Vec<u64> as FromSql<'_>>::accepts(&mysql_uint8_array));
    assert!(<Vec<u64> as ToSql>::accepts(&mysql_uint8_array));
}

#[test]
fn mysql_sql_aliases_use_postgresql_canonical_types() {
    for (oid, expected) in [
        (21, Type::INT2),
        (23, Type::INT4),
        (20, Type::INT8),
        (701, Type::FLOAT8),
        (16, Type::BOOL),
    ] {
        assert_eq!(Type::from_oid_in(TypeSystem::Mysql, oid), Some(expected));
    }
}
