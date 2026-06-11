//! Conversions to and from Postgres types.
//!
//! This crate is used by the `tokio-postgres` and `postgres` crates. You normally don't need to depend directly on it
//! unless you want to define your own `ToSql` or `FromSql` definitions.
//!
//! # Derive
//!
//! If the `derive` cargo feature is enabled, you can derive `ToSql` and `FromSql` implementations for custom Postgres
//! types. Explicitly, modify your `Cargo.toml` file to include the following:
//!
//! ```toml
//! [dependencies]
//! postgres-types = { version = "0.X.X", features = ["derive"] }
//! ```
//!
//! ## Enums
//!
//! Postgres enums correspond to C-like enums in Rust:
//!
//! ```sql
//! CREATE TYPE "Mood" AS ENUM (
//!     'Sad',
//!     'Ok',
//!     'Happy'
//! );
//! ```
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! enum Mood {
//!     Sad,
//!     Ok,
//!     Happy,
//! }
//! ```
//!
//! ## Domains
//!
//! Postgres domains correspond to tuple structs with one member in Rust:
//!
//! ```sql
//! CREATE DOMAIN "SessionId" AS BYTEA CHECK(octet_length(VALUE) = 16);
//! ```
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! struct SessionId(Vec<u8>);
//! ```
//!
//! ## Newtypes
//!
//! The `#[postgres(transparent)]` attribute can be used on a single-field tuple struct to create a
//! Rust-only wrapper type that will use the [`ToSql`] & [`FromSql`] implementation of the inner
//! value :
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! #[postgres(transparent)]
//! struct UserId(i32);
//! ```
//!
//! ## Composites
//!
//! Postgres composite types correspond to structs in Rust:
//!
//! ```sql
//! CREATE TYPE "InventoryItem" AS (
//!     name TEXT,
//!     supplier_id INT,
//!     price DOUBLE PRECISION
//! );
//! ```
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! struct InventoryItem {
//!     name: String,
//!     supplier_id: i32,
//!     price: Option<f64>,
//! }
//! ```
//!
//! ## Naming
//!
//! The derived implementations will enforce exact matches of type, field, and variant names between the Rust and
//! Postgres types. The `#[postgres(name = "...")]` attribute can be used to adjust the name on a type, variant, or
//! field:
//!
//! ```sql
//! CREATE TYPE mood AS ENUM (
//!     'sad',
//!     'ok',
//!     'happy'
//! );
//! ```
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! #[postgres(name = "mood")]
//! enum Mood {
//!     #[postgres(name = "sad")]
//!     Sad,
//!     #[postgres(name = "ok")]
//!     Ok,
//!     #[postgres(name = "happy")]
//!     Happy,
//! }
//! ```
//!
//! Alternatively, the `#[postgres(rename_all = "...")]` attribute can be used to rename all fields or variants
//! with the chosen casing convention. This will not affect the struct or enum's type name. Note that
//! `#[postgres(name = "...")]` takes precendence when used in conjunction with `#[postgres(rename_all = "...")]`:
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! #[postgres(name = "mood", rename_all = "snake_case")]
//! enum Mood {
//!     #[postgres(name = "ok")]
//!     Ok,             // ok
//!     VeryHappy,      // very_happy
//! }
//! ```
//!
//! The following case conventions are supported:
//! - `"lowercase"`
//! - `"UPPERCASE"`
//! - `"PascalCase"`
//! - `"camelCase"`
//! - `"snake_case"`
//! - `"SCREAMING_SNAKE_CASE"`
//! - `"kebab-case"`
//! - `"SCREAMING-KEBAB-CASE"`
//! - `"Train-Case"`
//!
//! ## Allowing Enum Mismatches
//!
//! By default the generated implementation of [`ToSql`] & [`FromSql`] for enums will require an exact match of the enum
//! variants between the Rust and Postgres types.
//! To allow mismatches, the `#[postgres(allow_mismatch)]` attribute can be used on the enum definition:
//!
//! ```sql
//! CREATE TYPE mood AS ENUM (
//!   'Sad',
//!   'Ok',
//!   'Happy'
//! );
//! ```
//!
//! ```rust
//! # #[cfg(feature = "derive")]
//! use postgres_types::{ToSql, FromSql};
//!
//! # #[cfg(feature = "derive")]
//! #[derive(Debug, ToSql, FromSql)]
//! #[postgres(allow_mismatch)]
//! enum Mood {
//!    Happy,
//!    Meh,
//! }
//! ```
#![warn(clippy::all, rust_2018_idioms, missing_docs)]
use fallible_iterator::FallibleIterator;
use postgres_protocol::types::{self, ArrayDimension};
use std::any::type_name;
use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::hash::BuildHasher;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(feature = "derive")]
pub use postgres_derive::{FromSql, ToSql};

#[cfg(feature = "with-serde_json-1")]
pub use crate::serde_json_1::Json;
use crate::type_gen::{Inner, Other};

#[doc(inline)]
pub use postgres_protocol::Oid;

#[doc(inline)]
pub use pg_lsn::PgLsn;

pub use crate::special::{Date, Timestamp};
use bytes::BytesMut;

// Number of seconds from 1970-01-01 to 2000-01-01
const TIME_SEC_CONVERSION: u64 = 946_684_800;
const USEC_PER_SEC: u64 = 1_000_000;
const NSEC_PER_USEC: u64 = 1_000;

/// Generates a simple implementation of `ToSql::accepts` which accepts the
/// types passed to it.
#[macro_export]
macro_rules! accepts {
    ($($expected:ident),+) => (
        fn accepts(ty: &$crate::Type) -> bool {
            false $(|| ty.is_equivalent_to(&$crate::Type::$expected))+
        }
    )
}

/// Generates an implementation of `ToSql::to_sql_checked`.
///
/// All `ToSql` implementations should use this macro.
#[macro_export]
macro_rules! to_sql_checked {
    () => {
        fn to_sql_checked(
            &self,
            ty: &$crate::Type,
            out: &mut $crate::private::BytesMut,
        ) -> ::std::result::Result<
            $crate::IsNull,
            Box<dyn ::std::error::Error + ::std::marker::Sync + ::std::marker::Send>,
        > {
            $crate::__to_sql_checked(self, ty, out)
        }
    };
}

// WARNING: this function is not considered part of this crate's public API.
// It is subject to change at any time.
#[doc(hidden)]
pub fn __to_sql_checked<T>(
    v: &T,
    ty: &Type,
    out: &mut BytesMut,
) -> Result<IsNull, Box<dyn Error + Sync + Send>>
where
    T: ToSql,
{
    if !T::accepts(ty) {
        return Err(Box::new(WrongType::new::<T>(ty.clone())));
    }
    v.to_sql(ty, out)
}

#[cfg(feature = "with-bigdecimal-0_4")]
mod bigdecimal_04;
#[cfg(feature = "with-bit-vec-0_6")]
mod bit_vec_06;
#[cfg(feature = "with-bit-vec-0_7")]
mod bit_vec_07;
#[cfg(feature = "with-bit-vec-0_8")]
mod bit_vec_08;
#[cfg(feature = "with-bit-vec-0_9")]
mod bit_vec_09;
#[cfg(feature = "with-chrono-0_4")]
mod chrono_04;
#[cfg(feature = "with-cidr-0_2")]
mod cidr_02;
#[cfg(feature = "with-cidr-0_3")]
mod cidr_03;
#[cfg(feature = "with-eui48-0_4")]
mod eui48_04;
#[cfg(feature = "with-eui48-1")]
mod eui48_1;
#[cfg(feature = "with-geo-types-0_6")]
mod geo_types_06;
#[cfg(feature = "with-geo-types-0_7")]
mod geo_types_07;
#[cfg(feature = "with-jiff-0_1")]
mod jiff_01;
#[cfg(feature = "with-jiff-0_2")]
mod jiff_02;
#[cfg(feature = "with-serde_json-1")]
mod serde_json_1;
#[cfg(feature = "with-smol_str-01")]
mod smol_str_01;
#[cfg(feature = "with-time-0_2")]
mod time_02;
#[cfg(feature = "with-time-0_3")]
mod time_03;
#[cfg(feature = "with-uuid-0_8")]
mod uuid_08;
#[cfg(feature = "with-uuid-1")]
mod uuid_1;

// The time::{date, time} macros produce compile errors if the crate package is renamed.
#[cfg(feature = "with-time-0_2")]
extern crate time_02 as time;

pub mod kingbase;
mod mysql_type_gen;
mod pg_lsn;
mod pg_type_gen;
#[doc(hidden)]
pub mod private;
mod special;
mod type_gen;

/// A Kingbase wire-protocol type table.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TypeSystem {
    /// PostgreSQL-compatible type table.
    Pg,
    /// Kingbase MySQL-compatible type table.
    Mysql,
}

/// A Kingbase type.
#[derive(PartialEq, Eq, Clone, Hash)]
pub struct Type(Inner);

impl fmt::Debug for Type {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.schema() {
            "public" | "pg_catalog" => {}
            schema => write!(fmt, "{schema}.")?,
        }
        fmt.write_str(self.name())
    }
}

impl Type {
    /// Creates a new `Type`.
    pub fn new(name: String, oid: Oid, kind: Kind, schema: String) -> Type {
        Type(Inner::Other(Arc::new(Other {
            name,
            oid,
            kind,
            schema,
        })))
    }

    /// Returns the `Type` corresponding to the provided `Oid` if it
    /// corresponds to a built-in type.
    pub fn from_oid(oid: Oid) -> Option<Type> {
        Type::from_pg_oid(oid)
    }

    /// Returns the PostgreSQL-compatible built-in type for an OID.
    pub fn from_pg_oid(oid: Oid) -> Option<Type> {
        pg_type_gen::Inner::from_oid(oid).map(|inner| Type(Inner::Pg(inner)))
    }

    /// Returns the built-in `Type` for an `Oid` in a specific type system.
    pub fn from_oid_in(type_system: TypeSystem, oid: Oid) -> Option<Type> {
        match type_system {
            TypeSystem::Pg => Self::from_pg_oid(oid),
            TypeSystem::Mysql => Self::from_mysql_oid(oid),
        }
    }

    /// Returns the Kingbase MySQL-compatible built-in type for an OID.
    pub fn from_mysql_oid(oid: Oid) -> Option<Type> {
        mysql_type_gen::Inner::from_oid(oid).map(|inner| Type(Inner::Mysql(inner)))
    }

    /// Returns the `Type` corresponding to the provided Kingbase MySQL-compatible
    /// `Oid` if it corresponds to a built-in type in that compatibility mode.
    ///
    /// This intentionally does not affect [`Type::from_oid`], which remains scoped
    /// to PostgreSQL built-in types. Common OIDs may exist in both generated
    /// tables, but this method returns the Kingbase MySQL variant.
    pub fn from_kingbase_mysql_oid(oid: Oid) -> Option<Type> {
        Self::from_mysql_oid(oid)
    }

    /// Returns the built-in type system this type belongs to, if known.
    pub fn type_system(&self) -> Option<TypeSystem> {
        match self.0 {
            Inner::Pg(_) => Some(TypeSystem::Pg),
            Inner::Mysql(_) => Some(TypeSystem::Mysql),
            Inner::Other(_) => None,
        }
    }

    /// Returns true when this type can use the same Rust conversion as `expected`.
    pub fn is_equivalent_to(&self, expected: &Type) -> bool {
        if self == expected {
            return true;
        }

        match self.0 {
            Inner::Mysql(_) => {
                if self.oid() == expected.oid()
                    && self.name() == expected.name()
                    && self.schema() == expected.schema()
                {
                    return true;
                }

                matches!(self.kind(), Kind::Domain(base) if base.is_equivalent_to(expected))
            }
            _ => false,
        }
    }

    /// Returns the OID of the `Type`.
    pub fn oid(&self) -> Oid {
        self.0.oid()
    }

    /// Returns the kind of this type.
    pub fn kind(&self) -> &Kind {
        self.0.kind()
    }

    /// Returns the schema of this type.
    pub fn schema(&self) -> &str {
        self.0.schema()
    }

    /// Returns the name of this type.
    pub fn name(&self) -> &str {
        self.0.name()
    }
}

#[cfg(test)]
mod tests {
    use super::{FromSql, Kind, ToSql, Type};
    use crate::kingbase::{MySqlBit, TinyInt};

    #[test]
    fn generated_type_modes_dispatch_by_schema() {
        let int4 = Type::from_oid(23).unwrap();
        assert_eq!(int4, Type::INT4);
        assert_eq!(int4.name(), "int4");
        assert_eq!(int4.schema(), "pg_catalog");

        let mysql_binary = Type::from_kingbase_mysql_oid(3383).unwrap();
        assert_eq!(mysql_binary.name(), "binary");
        assert_eq!(mysql_binary.schema(), "sys");

        let mysql_int4 = Type::from_kingbase_mysql_oid(23).unwrap();
        assert_ne!(mysql_int4, Type::INT4);
        assert_eq!(mysql_int4.oid(), Type::INT4.oid());
        assert_eq!(mysql_int4.name(), "int4");
        assert_eq!(mysql_int4.schema(), "pg_catalog");

        let mysql_date = Type::from_kingbase_mysql_oid(1082).unwrap();
        assert_eq!(mysql_date, Type::MYSQL_DATE);
        assert_eq!(mysql_date.schema(), "pg_catalog");

        let mysql_sys_date = Type::from_kingbase_mysql_oid(7944).unwrap();
        assert_eq!(mysql_sys_date.name(), "date");
        assert_eq!(mysql_sys_date.schema(), "sys");
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
        assert!(<i64 as ToSql>::accepts(&Type::from_mysql_oid(20).unwrap()));
        assert!(<f32 as ToSql>::accepts(&Type::from_mysql_oid(700).unwrap()));
        assert!(<f64 as ToSql>::accepts(&Type::from_mysql_oid(701).unwrap()));

        assert!(<String as FromSql<'_>>::accepts(
            &Type::from_mysql_oid(25).unwrap()
        ));
        assert!(<&str as ToSql>::accepts(
            &Type::from_mysql_oid(1043).unwrap()
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
    fn mysql_mode_arrays_use_mysql_member_types() {
        let mysql_int4_array = Type::from_mysql_oid(1007).unwrap();
        assert_eq!(mysql_int4_array, Type::MYSQL_INT4_ARRAY);
        match mysql_int4_array.kind() {
            Kind::Array(member) => assert_eq!(member, &Type::MYSQL_INT4),
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
        assert!(<Vec<TinyInt> as FromSql<'_>>::accepts(&mysql_tinyint_array));
        assert!(<Vec<TinyInt> as ToSql>::accepts(&mysql_tinyint_array));
    }

    #[test]
    fn mysql_sql_alias_constants_share_canonical_types() {
        assert_eq!(Type::MYSQL_SMALLINT, Type::MYSQL_INT2);
        assert_eq!(Type::MYSQL_SMALLINT_ARRAY, Type::MYSQL_INT2_ARRAY);
        assert!(<i16 as ToSql>::accepts(&Type::MYSQL_SMALLINT));
        assert!(<Vec<i16> as ToSql>::accepts(&Type::MYSQL_SMALLINT_ARRAY));

        assert_eq!(Type::MYSQL_INTEGER, Type::MYSQL_INT4);
        assert_eq!(Type::MYSQL_INT, Type::MYSQL_INT4);
        assert_eq!(Type::MYSQL_INTEGER_ARRAY, Type::MYSQL_INT4_ARRAY);
        assert_eq!(Type::MYSQL_INT_ARRAY, Type::MYSQL_INT4_ARRAY);
        assert!(<i32 as ToSql>::accepts(&Type::MYSQL_INTEGER));
        assert!(<Vec<i32> as ToSql>::accepts(&Type::MYSQL_INT_ARRAY));

        assert_eq!(Type::MYSQL_BIGINT, Type::MYSQL_INT8);
        assert_eq!(Type::MYSQL_BIGINT_ARRAY, Type::MYSQL_INT8_ARRAY);
        assert!(<i64 as ToSql>::accepts(&Type::MYSQL_BIGINT));
        assert!(<Vec<i64> as ToSql>::accepts(&Type::MYSQL_BIGINT_ARRAY));

        assert_eq!(Type::MYSQL_DOUBLE, Type::MYSQL_FLOAT8);
        assert_eq!(Type::MYSQL_DOUBLE_ARRAY, Type::MYSQL_FLOAT8_ARRAY);
        assert!(<f64 as ToSql>::accepts(&Type::MYSQL_DOUBLE));
        assert!(<Vec<f64> as ToSql>::accepts(&Type::MYSQL_DOUBLE_ARRAY));

        assert_eq!(Type::MYSQL_BOOLEAN, Type::MYSQL_BOOL);
        assert_eq!(Type::MYSQL_BOOLEAN_ARRAY, Type::MYSQL_BOOL_ARRAY);
        assert!(<bool as ToSql>::accepts(&Type::MYSQL_BOOLEAN));
        assert!(<Vec<bool> as ToSql>::accepts(&Type::MYSQL_BOOLEAN_ARRAY));
    }
}

/// Represents the kind of a Postgres type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A simple type like `VARCHAR` or `INTEGER`.
    Simple,
    /// An enumerated type along with its variants.
    Enum(Vec<String>),
    /// A pseudo-type.
    Pseudo,
    /// An array type along with the type of its elements.
    Array(Type),
    /// A range type along with the type of its elements.
    Range(Type),
    /// A multirange type along with the type of its elements.
    Multirange(Type),
    /// A domain type along with its underlying type.
    Domain(Type),
    /// A composite type along with information about its fields.
    Composite(Vec<Field>),
}

/// Information about a field of a composite type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Field {
    name: String,
    type_: Type,
}

impl Field {
    /// Creates a new `Field`.
    pub fn new(name: String, type_: Type) -> Field {
        Field { name, type_ }
    }

    /// Returns the name of the field.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the type of the field.
    pub fn type_(&self) -> &Type {
        &self.type_
    }
}

/// An error indicating that a `NULL` Postgres value was passed to a `FromSql`
/// implementation that does not support `NULL` values.
#[derive(Debug, Clone, Copy)]
pub struct WasNull;

impl fmt::Display for WasNull {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("a Postgres value was `NULL`")
    }
}

impl Error for WasNull {}

/// An error indicating that a conversion was attempted between incompatible
/// Rust and Postgres types.
#[derive(Debug)]
pub struct WrongType {
    postgres: Type,
    rust: &'static str,
}

impl fmt::Display for WrongType {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "cannot convert between the Rust type `{}` and the Postgres type `{}`",
            self.rust, self.postgres,
        )
    }
}

impl Error for WrongType {}

impl WrongType {
    /// Creates a new `WrongType` error.
    pub fn new<T>(ty: Type) -> WrongType {
        WrongType {
            postgres: ty,
            rust: type_name::<T>(),
        }
    }
}

/// A trait for types that can be created from a Postgres value.
///
/// # Types
///
/// The following implementations are provided by this crate, along with the
/// corresponding Postgres types:
///
/// | Rust type                         | Postgres type(s)                              |
/// |-----------------------------------|-----------------------------------------------|
/// | `bool`                            | BOOL                                          |
/// | `i8`                              | "char"                                        |
/// | `i16`                             | SMALLINT, SMALLSERIAL                         |
/// | `i32`                             | INT, SERIAL                                   |
/// | `u32`                             | OID                                           |
/// | `i64`                             | BIGINT, BIGSERIAL                             |
/// | `f32`                             | REAL                                          |
/// | `f64`                             | DOUBLE PRECISION                              |
/// | `&str`/`String`                   | VARCHAR, CHAR(n), TEXT, CITEXT, NAME, UNKNOWN |
/// |                                   | LTREE, LQUERY, LTXTQUERY                      |
/// | `&[u8]`/`Vec<u8>`                 | BYTEA                                         |
/// | `HashMap<String, Option<String>>` | HSTORE                                        |
/// | `SystemTime`                      | TIMESTAMP, TIMESTAMP WITH TIME ZONE           |
/// | `IpAddr`                          | INET                                          |
///
/// In addition, some implementations are provided for types in third party
/// crates. These are disabled by default; to opt into one of these
/// implementations, activate the Cargo feature corresponding to the crate's
/// name prefixed by `with-`. For example, the `with-serde_json-1` feature enables
/// the implementation for the `serde_json::Value` type.
///
/// | Rust type                       | Postgres type(s)                    |
/// |---------------------------------|-------------------------------------|
/// | `chrono::NaiveDateTime`         | TIMESTAMP                           |
/// | `chrono::DateTime<Utc>`         | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::DateTime<Local>`       | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::DateTime<FixedOffset>` | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::NaiveDate`             | DATE                                |
/// | `chrono::NaiveTime`             | TIME                                |
/// | `cidr::IpCidr`                  | CIDR                                |
/// | `cidr::IpInet`                  | INET                                |
/// | `time::PrimitiveDateTime`       | TIMESTAMP                           |
/// | `time::OffsetDateTime`          | TIMESTAMP WITH TIME ZONE            |
/// | `time::Date`                    | DATE                                |
/// | `time::Time`                    | TIME                                |
/// | `jiff::civil::Date`             | DATE                                |
/// | `jiff::civil::DateTime`         | TIMESTAMP                           |
/// | `jiff::civil::Time`             | TIME                                |
/// | `jiff::Timestamp`               | TIMESTAMP WITH TIME ZONE            |
/// | `eui48::MacAddress`             | MACADDR                             |
/// | `geo_types::Point<f64>`         | POINT                               |
/// | `geo_types::Rect<f64>`          | BOX                                 |
/// | `geo_types::LineString<f64>`    | PATH                                |
/// | `serde_json::Value`             | JSON, JSONB                         |
/// | `uuid::Uuid`                    | UUID                                |
/// | `bit_vec::BitVec`               | BIT, VARBIT                         |
/// | `eui48::MacAddress`             | MACADDR                             |
/// | `cidr::InetCidr`                | CIDR                                |
/// | `cidr::InetAddr`                | INET                                |
/// | `smol_str::SmolStr`             | VARCHAR, CHAR(n), TEXT, CITEXT,     |
/// |                                 | NAME, UNKNOWN, LTREE, LQUERY,       |
/// |                                 | LTXTQUERY                           |
///
/// # Nullability
///
/// In addition to the types listed above, `FromSql` is implemented for
/// `Option<T>` where `T` implements `FromSql`. An `Option<T>` represents a
/// nullable Postgres value.
///
/// # Arrays
///
/// `FromSql` is implemented for `Vec<T>`, `Box<[T]>` and `[T; N]` where `T`
/// implements `FromSql`, and corresponds to one-dimensional Postgres arrays.
///
/// **Note:** the impl for arrays only exist when the Cargo feature `array-impls`
/// is enabled.
pub trait FromSql<'a>: Sized {
    /// Creates a new value of this type from a buffer of data of the specified
    /// Postgres `Type` in its binary format.
    ///
    /// The caller of this method is responsible for ensuring that this type
    /// is compatible with the Postgres `Type`.
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>>;

    /// Creates a new value of this type from a `NULL` SQL value.
    ///
    /// The caller of this method is responsible for ensuring that this type
    /// is compatible with the Postgres `Type`.
    ///
    /// The default implementation returns `Err(Box::new(WasNull))`.
    #[allow(unused_variables)]
    fn from_sql_null(ty: &Type) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Err(Box::new(WasNull))
    }

    /// A convenience function that delegates to `from_sql` and `from_sql_null` depending on the
    /// value of `raw`.
    fn from_sql_nullable(
        ty: &Type,
        raw: Option<&'a [u8]>,
    ) -> Result<Self, Box<dyn Error + Sync + Send>> {
        match raw {
            Some(raw) => Self::from_sql(ty, raw),
            None => Self::from_sql_null(ty),
        }
    }

    /// Determines if a value of this type can be created from the specified
    /// Postgres `Type`.
    fn accepts(ty: &Type) -> bool;
}

/// A trait for types which can be created from a Postgres value without borrowing any data.
///
/// This is primarily useful for trait bounds on functions.
pub trait FromSqlOwned: for<'a> FromSql<'a> {}

impl<T> FromSqlOwned for T where T: for<'a> FromSql<'a> {}

impl<'a, T: FromSql<'a>> FromSql<'a> for Option<T> {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Option<T>, Box<dyn Error + Sync + Send>> {
        <T as FromSql>::from_sql(ty, raw).map(Some)
    }

    fn from_sql_null(_: &Type) -> Result<Option<T>, Box<dyn Error + Sync + Send>> {
        Ok(None)
    }

    fn accepts(ty: &Type) -> bool {
        <T as FromSql>::accepts(ty)
    }
}

impl<'a, T: FromSql<'a>> FromSql<'a> for Vec<T> {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Vec<T>, Box<dyn Error + Sync + Send>> {
        let member_type = match *ty.kind() {
            Kind::Array(ref member) => member,
            _ => panic!("expected array type"),
        };

        let array = types::array_from_sql(raw)?;
        if array.dimensions().count()? > 1 {
            return Err("array contains too many dimensions".into());
        }

        array
            .values()
            .map(|v| T::from_sql_nullable(member_type, v))
            .collect()
    }

    fn accepts(ty: &Type) -> bool {
        match *ty.kind() {
            Kind::Array(ref inner) => T::accepts(inner),
            _ => false,
        }
    }
}

#[cfg(feature = "array-impls")]
impl<'a, T: FromSql<'a>, const N: usize> FromSql<'a> for [T; N] {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        let member_type = match *ty.kind() {
            Kind::Array(ref member) => member,
            _ => panic!("expected array type"),
        };

        let array = types::array_from_sql(raw)?;
        if array.dimensions().count()? > 1 {
            return Err("array contains too many dimensions".into());
        }

        let mut values = array.values();
        let out = array_init::try_array_init(|i| {
            let v = values
                .next()?
                .ok_or_else(|| -> Box<dyn Error + Sync + Send> {
                    format!("too few elements in array (expected {N}, got {i})").into()
                })?;
            T::from_sql_nullable(member_type, v)
        })?;
        if values.next()?.is_some() {
            return Err(
                format!("excess elements in array (expected {N}, got more than that)",).into(),
            );
        }

        Ok(out)
    }

    fn accepts(ty: &Type) -> bool {
        match *ty.kind() {
            Kind::Array(ref inner) => T::accepts(inner),
            _ => false,
        }
    }
}

impl<'a, T: FromSql<'a>> FromSql<'a> for Box<T> {
    fn from_sql(ty: &Type, row: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        T::from_sql(ty, row).map(Box::new)
    }

    fn accepts(ty: &Type) -> bool {
        T::accepts(ty)
    }
}

impl<'a, T: FromSql<'a>> FromSql<'a> for Box<[T]> {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Vec::<T>::from_sql(ty, raw).map(Vec::into_boxed_slice)
    }

    fn accepts(ty: &Type) -> bool {
        Vec::<T>::accepts(ty)
    }
}

impl<'a> FromSql<'a> for Vec<u8> {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Vec<u8>, Box<dyn Error + Sync + Send>> {
        Ok(types::bytea_from_sql(raw).to_owned())
    }

    accepts!(
        BYTEA,
        MYSQL_BYTEA,
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB
    );
}

impl<'a> FromSql<'a> for &'a [u8] {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<&'a [u8], Box<dyn Error + Sync + Send>> {
        Ok(types::bytea_from_sql(raw))
    }

    accepts!(
        BYTEA,
        MYSQL_BYTEA,
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB
    );
}

impl<'a> FromSql<'a> for String {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<String, Box<dyn Error + Sync + Send>> {
        <&str as FromSql>::from_sql(ty, raw).map(ToString::to_string)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as FromSql>::accepts(ty)
    }
}

impl<'a> FromSql<'a> for Box<str> {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Box<str>, Box<dyn Error + Sync + Send>> {
        <&str as FromSql>::from_sql(ty, raw)
            .map(ToString::to_string)
            .map(String::into_boxed_str)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as FromSql>::accepts(ty)
    }
}

impl<'a> FromSql<'a> for &'a str {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<&'a str, Box<dyn Error + Sync + Send>> {
        match *ty {
            ref ty if ty.name() == "ltree" => types::ltree_from_sql(raw),
            ref ty if ty.name() == "lquery" => types::lquery_from_sql(raw),
            ref ty if ty.name() == "ltxtquery" => types::ltxtquery_from_sql(raw),
            _ => types::text_from_sql(raw),
        }
    }

    fn accepts(ty: &Type) -> bool {
        ty.is_equivalent_to(&Type::VARCHAR)
            || ty.is_equivalent_to(&Type::TEXT)
            || ty.is_equivalent_to(&Type::BPCHAR)
            || ty.is_equivalent_to(&Type::NAME)
            || ty.is_equivalent_to(&Type::UNKNOWN)
            || ty.is_equivalent_to(&Type::MYSQL_VARCHAR)
            || ty.is_equivalent_to(&Type::MYSQL_TEXT)
            || ty.is_equivalent_to(&Type::MYSQL_BPCHAR)
            || ty.is_equivalent_to(&Type::MYSQL_NAME)
            || ty.is_equivalent_to(&Type::MYSQL_UNKNOWN)
            || ty.is_equivalent_to(&Type::MYSQL_LONGTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_MEDIUMTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_TINYTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_CLOB)
            || ty.is_equivalent_to(&Type::MYSQL_NCLOB)
            || ty.is_equivalent_to(&Type::MYSQL_BPCHARBYTE)
            || ty.is_equivalent_to(&Type::MYSQL_VARCHARBYTE)
            || matches!(ty.name(), "citext" | "ltree" | "lquery" | "ltxtquery")
    }
}

impl<'a> FromSql<'a> for Cow<'a, str> {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Cow<'a, str>, Box<dyn Error + Sync + Send>> {
        <&str as FromSql>::from_sql(ty, raw).map(Cow::Borrowed)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as FromSql>::accepts(ty)
    }
}

macro_rules! simple_from {
    ($t:ty, $f:ident, $($expected:ident),+) => {
        impl<'a> FromSql<'a> for $t {
            fn from_sql(_: &Type, raw: &'a [u8]) -> Result<$t, Box<dyn Error + Sync + Send>> {
                types::$f(raw)
            }

            accepts!($($expected),+);
        }
    }
}

simple_from!(bool, bool_from_sql, BOOL, MYSQL_BOOL);
simple_from!(i8, char_from_sql, CHAR, MYSQL_TINYINT);
simple_from!(i16, int2_from_sql, INT2, MYSQL_INT2);
simple_from!(
    i32,
    int4_from_sql,
    INT4,
    MYSQL_INT4,
    MYSQL_INT3,
    MYSQL_MEDIUMINT,
    MYSQL_MIDDLEINT
);
simple_from!(u32, oid_from_sql, OID, MYSQL_UINT4);
simple_from!(i64, int8_from_sql, INT8, MYSQL_INT8);
simple_from!(f32, float4_from_sql, FLOAT4, MYSQL_FLOAT4);
simple_from!(f64, float8_from_sql, FLOAT8, MYSQL_FLOAT8);

impl<'a, S> FromSql<'a> for HashMap<String, Option<String>, S>
where
    S: Default + BuildHasher,
{
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> Result<HashMap<String, Option<String>, S>, Box<dyn Error + Sync + Send>> {
        types::hstore_from_sql(raw)?
            .map(|(k, v)| Ok((k.to_owned(), v.map(str::to_owned))))
            .collect()
    }

    fn accepts(ty: &Type) -> bool {
        ty.name() == "hstore"
    }
}

impl<'a> FromSql<'a> for SystemTime {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<SystemTime, Box<dyn Error + Sync + Send>> {
        let time = types::timestamp_from_sql(raw)?;
        let epoch = UNIX_EPOCH + Duration::from_secs(TIME_SEC_CONVERSION);

        let negative = time < 0;
        let time = time.unsigned_abs();

        let secs = time / USEC_PER_SEC;
        let nsec = (time % USEC_PER_SEC) * NSEC_PER_USEC;
        let offset = Duration::new(secs, nsec as u32);

        let time = if negative {
            epoch - offset
        } else {
            epoch + offset
        };

        Ok(time)
    }

    accepts!(
        TIMESTAMP,
        TIMESTAMPTZ,
        MYSQL_TIMESTAMP,
        MYSQL_TIMESTAMPTZ,
        MYSQL_DATETIME
    );
}

impl<'a> FromSql<'a> for IpAddr {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<IpAddr, Box<dyn Error + Sync + Send>> {
        let inet = types::inet_from_sql(raw)?;
        Ok(inet.addr())
    }

    accepts!(INET);
}

/// An enum representing the nullability of a Postgres value.
pub enum IsNull {
    /// The value is NULL.
    Yes,
    /// The value is not NULL.
    No,
}

/// A trait for types that can be converted into Postgres values.
///
/// # Types
///
/// The following implementations are provided by this crate, along with the
/// corresponding Postgres types:
///
/// | Rust type                         | Postgres type(s)                     |
/// |-----------------------------------|--------------------------------------|
/// | `bool`                            | BOOL                                 |
/// | `i8`                              | "char"                               |
/// | `i16`                             | SMALLINT, SMALLSERIAL                |
/// | `i32`                             | INT, SERIAL                          |
/// | `u32`                             | OID                                  |
/// | `i64`                             | BIGINT, BIGSERIAL                    |
/// | `f32`                             | REAL                                 |
/// | `f64`                             | DOUBLE PRECISION                     |
/// | `&str`/`String`                   | VARCHAR, CHAR(n), TEXT, CITEXT, NAME |
/// |                                   | LTREE, LQUERY, LTXTQUERY             |
/// | `&[u8]`/`Vec<u8>`/`[u8; N]`       | BYTEA                                |
/// | `HashMap<String, Option<String>>` | HSTORE                               |
/// | `SystemTime`                      | TIMESTAMP, TIMESTAMP WITH TIME ZONE  |
/// | `IpAddr`                          | INET                                 |
///
/// In addition, some implementations are provided for types in third party
/// crates. These are disabled by default; to opt into one of these
/// implementations, activate the Cargo feature corresponding to the crate's
/// name prefixed by `with-`. For example, the `with-serde_json-1` feature enables
/// the implementation for the `serde_json::Value` type.
///
/// | Rust type                       | Postgres type(s)                    |
/// |---------------------------------|-------------------------------------|
/// | `chrono::NaiveDateTime`         | TIMESTAMP                           |
/// | `chrono::DateTime<Utc>`         | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::DateTime<Local>`       | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::DateTime<FixedOffset>` | TIMESTAMP WITH TIME ZONE            |
/// | `chrono::NaiveDate`             | DATE                                |
/// | `chrono::NaiveTime`             | TIME                                |
/// | `cidr::IpCidr`                  | CIDR                                |
/// | `cidr::IpInet`                  | INET                                |
/// | `time::PrimitiveDateTime`       | TIMESTAMP                           |
/// | `time::OffsetDateTime`          | TIMESTAMP WITH TIME ZONE            |
/// | `time::Date`                    | DATE                                |
/// | `time::Time`                    | TIME                                |
/// | `eui48::MacAddress`             | MACADDR                             |
/// | `geo_types::Point<f64>`         | POINT                               |
/// | `geo_types::Rect<f64>`          | BOX                                 |
/// | `geo_types::LineString<f64>`    | PATH                                |
/// | `serde_json::Value`             | JSON, JSONB                         |
/// | `uuid::Uuid`                    | UUID                                |
/// | `bit_vec::BitVec`               | BIT, VARBIT                         |
/// | `eui48::MacAddress`             | MACADDR                             |
///
/// # Nullability
///
/// In addition to the types listed above, `ToSql` is implemented for
/// `Option<T>` where `T` implements `ToSql`. An `Option<T>` represents a
/// nullable Postgres value.
///
/// # Arrays
///
/// `ToSql` is implemented for `[u8; N]`, `Vec<T>`, `&[T]`, `Box<[T]>` and `[T; N]`
/// where `T` implements `ToSql` and `N` is const usize, and corresponds to one-dimensional
/// Postgres arrays with an index offset of 1.
/// To make conversion work correctly for `WHERE ... IN` clauses, for example
/// `WHERE col IN ($1)`, you may instead have to use the construct
/// `WHERE col = ANY ($1)` which expects an array.
///
/// **Note:** the impl for arrays only exist when the Cargo feature `array-impls`
/// is enabled.
pub trait ToSql: fmt::Debug {
    /// Converts the value of `self` into the binary format of the specified
    /// Postgres `Type`, appending it to `out`.
    ///
    /// The caller of this method is responsible for ensuring that this type
    /// is compatible with the Postgres `Type`.
    ///
    /// The return value indicates if this value should be represented as
    /// `NULL`. If this is the case, implementations **must not** write
    /// anything to `out`.
    fn to_sql(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>>
    where
        Self: Sized;

    /// Determines if a value of this type can be converted to the specified
    /// Postgres `Type`.
    fn accepts(ty: &Type) -> bool
    where
        Self: Sized;

    /// An adaptor method used internally by Rust-Postgres.
    ///
    /// *All* implementations of this method should be generated by the
    /// `to_sql_checked!()` macro.
    fn to_sql_checked(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>>;

    /// Specify the encode format
    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Binary
    }

}

/// Supported Postgres message format types
///
/// Using Text format in a message assumes a Postgres `SERVER_ENCODING` of `UTF8`
#[derive(Clone, Copy, Debug)]
pub enum Format {
    /// Text format (UTF-8)
    Text,
    /// Compact, typed binary format
    Binary,
}

impl<T> ToSql for &T
where
    T: ToSql,
{
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        (*self).to_sql(ty, out)
    }

    fn accepts(ty: &Type) -> bool {
        T::accepts(ty)
    }

    fn encode_format(&self, ty: &Type) -> Format {
        (*self).encode_format(ty)
    }

    to_sql_checked!();
}

impl<T: ToSql> ToSql for Option<T> {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match *self {
            Some(ref val) => val.to_sql(ty, out),
            None => Ok(IsNull::Yes),
        }
    }

    fn accepts(ty: &Type) -> bool {
        <T as ToSql>::accepts(ty)
    }

    fn encode_format(&self, ty: &Type) -> Format {
        match self {
            Some(val) => val.encode_format(ty),
            None => Format::Binary,
        }
    }

    to_sql_checked!();
}

impl<T: ToSql> ToSql for &[T] {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let member_type = match *ty.kind() {
            Kind::Array(ref member) => member,
            _ => panic!("expected array type"),
        };

        // Arrays are normally one indexed by default but oidvector and int2vector *require* zero indexing
        let lower_bound = match *ty {
            Type::OID_VECTOR | Type::INT2_VECTOR => 0,
            _ => 1,
        };

        let dimension = ArrayDimension {
            len: downcast(self.len())?,
            lower_bound,
        };

        types::array_to_sql(
            Some(dimension),
            member_type.oid(),
            self.iter(),
            |e, w| match e.to_sql(member_type, w)? {
                IsNull::No => Ok(postgres_protocol::IsNull::No),
                IsNull::Yes => Ok(postgres_protocol::IsNull::Yes),
            },
            w,
        )?;
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        match *ty.kind() {
            Kind::Array(ref member) => T::accepts(member),
            _ => false,
        }
    }

    to_sql_checked!();
}

impl ToSql for &[u8] {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        types::bytea_to_sql(self, w);
        Ok(IsNull::No)
    }

    accepts!(
        BYTEA,
        MYSQL_BYTEA,
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB
    );

    to_sql_checked!();
}

#[cfg(feature = "array-impls")]
impl<const N: usize> ToSql for [u8; N] {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        types::bytea_to_sql(&self[..], w);
        Ok(IsNull::No)
    }

    accepts!(
        BYTEA,
        MYSQL_BYTEA,
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB
    );

    to_sql_checked!();
}

#[cfg(feature = "array-impls")]
impl<T: ToSql, const N: usize> ToSql for [T; N] {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&[T] as ToSql>::to_sql(&&self[..], ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&[T] as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl<T: ToSql> ToSql for Vec<T> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&[T] as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&[T] as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl<T: ToSql> ToSql for Box<T> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&T as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&T as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl<T: ToSql> ToSql for Box<[T]> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&[T] as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&[T] as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl ToSql for Cow<'_, [u8]> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&[u8] as ToSql>::to_sql(&self.as_ref(), ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&[u8] as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl ToSql for Vec<u8> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&[u8] as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&[u8] as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl ToSql for &str {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match ty.name() {
            "ltree" => types::ltree_to_sql(self, w),
            "lquery" => types::lquery_to_sql(self, w),
            "ltxtquery" => types::ltxtquery_to_sql(self, w),
            _ => types::text_to_sql(self, w),
        }
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty.is_equivalent_to(&Type::VARCHAR)
            || ty.is_equivalent_to(&Type::TEXT)
            || ty.is_equivalent_to(&Type::BPCHAR)
            || ty.is_equivalent_to(&Type::NAME)
            || ty.is_equivalent_to(&Type::UNKNOWN)
            || ty.is_equivalent_to(&Type::MYSQL_VARCHAR)
            || ty.is_equivalent_to(&Type::MYSQL_TEXT)
            || ty.is_equivalent_to(&Type::MYSQL_BPCHAR)
            || ty.is_equivalent_to(&Type::MYSQL_NAME)
            || ty.is_equivalent_to(&Type::MYSQL_UNKNOWN)
            || ty.is_equivalent_to(&Type::MYSQL_LONGTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_MEDIUMTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_TINYTEXT)
            || ty.is_equivalent_to(&Type::MYSQL_CLOB)
            || ty.is_equivalent_to(&Type::MYSQL_NCLOB)
            || ty.is_equivalent_to(&Type::MYSQL_BPCHARBYTE)
            || ty.is_equivalent_to(&Type::MYSQL_VARCHARBYTE)
            || matches!(ty.name(), "citext" | "ltree" | "lquery" | "ltxtquery")
    }

    to_sql_checked!();
}

impl ToSql for Cow<'_, str> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&str as ToSql>::to_sql(&self.as_ref(), ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl ToSql for String {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&str as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

impl ToSql for Box<str> {
    fn to_sql(&self, ty: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        <&str as ToSql>::to_sql(&&**self, ty, w)
    }

    fn accepts(ty: &Type) -> bool {
        <&str as ToSql>::accepts(ty)
    }

    to_sql_checked!();
}

macro_rules! simple_to {
    ($t:ty, $f:ident, $($expected:ident),+) => {
        impl ToSql for $t {
            fn to_sql(&self,
                      _: &Type,
                      w: &mut BytesMut)
                      -> Result<IsNull, Box<dyn Error + Sync + Send>> {
                types::$f(*self, w);
                Ok(IsNull::No)
            }

            accepts!($($expected),+);

            to_sql_checked!();
        }
    }
}

simple_to!(bool, bool_to_sql, BOOL, MYSQL_BOOL);
simple_to!(i8, char_to_sql, CHAR, MYSQL_TINYINT);
simple_to!(i16, int2_to_sql, INT2, MYSQL_INT2);
simple_to!(
    i32,
    int4_to_sql,
    INT4,
    MYSQL_INT4,
    MYSQL_INT3,
    MYSQL_MEDIUMINT,
    MYSQL_MIDDLEINT
);
simple_to!(u32, oid_to_sql, OID, MYSQL_UINT4);
simple_to!(i64, int8_to_sql, INT8, MYSQL_INT8);
simple_to!(f32, float4_to_sql, FLOAT4, MYSQL_FLOAT4);
simple_to!(f64, float8_to_sql, FLOAT8, MYSQL_FLOAT8);

impl<H> ToSql for HashMap<String, Option<String>, H>
where
    H: BuildHasher,
{
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        types::hstore_to_sql(
            self.iter().map(|(k, v)| (&**k, v.as_ref().map(|v| &**v))),
            w,
        )?;
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty.name() == "hstore"
    }

    to_sql_checked!();
}

impl ToSql for SystemTime {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let epoch = UNIX_EPOCH + Duration::from_secs(TIME_SEC_CONVERSION);

        let to_usec =
            |d: Duration| d.as_secs() * USEC_PER_SEC + u64::from(d.subsec_nanos()) / NSEC_PER_USEC;

        let time = match self.duration_since(epoch) {
            Ok(duration) => to_usec(duration) as i64,
            Err(e) => -(to_usec(e.duration()) as i64),
        };

        types::timestamp_to_sql(time, w);
        Ok(IsNull::No)
    }

    accepts!(
        TIMESTAMP,
        TIMESTAMPTZ,
        MYSQL_TIMESTAMP,
        MYSQL_TIMESTAMPTZ,
        MYSQL_DATETIME
    );

    to_sql_checked!();
}

impl ToSql for IpAddr {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let netmask = match self {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        types::inet_to_sql(*self, netmask, w);
        Ok(IsNull::No)
    }

    accepts!(INET);

    to_sql_checked!();
}

fn downcast(len: usize) -> Result<i32, Box<dyn Error + Sync + Send>> {
    if len > i32::MAX as usize {
        Err("value too large to transmit".into())
    } else {
        Ok(len as i32)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A trait used by clients to abstract over `&dyn ToSql` and `T: ToSql`.
///
/// This cannot be implemented outside of this crate.
pub trait BorrowToSql: sealed::Sealed {
    /// Returns a reference to `self` as a `ToSql` trait object.
    fn borrow_to_sql(&self) -> &dyn ToSql;
}

impl sealed::Sealed for &dyn ToSql {}

impl BorrowToSql for &dyn ToSql {
    #[inline]
    fn borrow_to_sql(&self) -> &dyn ToSql {
        *self
    }
}

impl sealed::Sealed for Box<dyn ToSql + Sync + '_> {}

impl BorrowToSql for Box<dyn ToSql + Sync + '_> {
    #[inline]
    fn borrow_to_sql(&self) -> &dyn ToSql {
        self.as_ref()
    }
}

impl sealed::Sealed for Box<dyn ToSql + Sync + Send + '_> {}
impl BorrowToSql for Box<dyn ToSql + Sync + Send + '_> {
    #[inline]
    fn borrow_to_sql(&self) -> &dyn ToSql {
        self.as_ref()
    }
}

impl sealed::Sealed for &(dyn ToSql + Sync) {}

/// In async contexts it is sometimes necessary to have the additional
/// Sync requirement on parameters for queries since this enables the
/// resulting Futures to be Send, hence usable in, e.g., tokio::spawn.
/// This instance is provided for those cases.
impl BorrowToSql for &(dyn ToSql + Sync) {
    #[inline]
    fn borrow_to_sql(&self) -> &dyn ToSql {
        *self
    }
}

impl<T> sealed::Sealed for T where T: ToSql {}

impl<T> BorrowToSql for T
where
    T: ToSql,
{
    #[inline]
    fn borrow_to_sql(&self) -> &dyn ToSql {
        self
    }
}
