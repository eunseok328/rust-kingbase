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
use bytes::{Buf, BufMut, BytesMut};

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
            matches!(*ty, $($crate::Type::$expected)|+)
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
mod oracle_type_gen;
mod pg_lsn;
mod pg_type_gen;
#[doc(hidden)]
pub mod private;
mod special;
mod sqlserver_type_gen;
mod type_gen;

/// A KingbaseES wire-protocol type table.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TypeSystem {
    /// PostgreSQL-compatible type table.
    Pg,
    /// KingbaseES MySQL-compatible type table.
    Mysql,
    /// KingbaseES Oracle-compatible type table.
    Oracle,
    /// KingbaseES SQL Server-compatible type table.
    SqlServer,
}

/// A KingbaseES type.
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
            // Compatibility extensions are checked first. Some Kingbase
            // modes intentionally reuse OIDs that are assigned to unrelated
            // PostgreSQL catalog types, so PG-first lookup would misclassify
            // those values. Shared types then fall back to the PG table.
            TypeSystem::Mysql => mysql_type_gen::Inner::from_oid(oid)
                .map(|inner| Type(Inner::Mysql(inner)))
                .or_else(|| Self::from_pg_oid(oid)),
            TypeSystem::Oracle => oracle_type_gen::Inner::from_oid(oid)
                .map(|inner| Type(Inner::Oracle(inner)))
                .or_else(|| Self::from_pg_oid(oid)),
            TypeSystem::SqlServer => sqlserver_type_gen::Inner::from_oid(oid)
                .map(|inner| Type(Inner::SqlServer(inner)))
                .or_else(|| Self::from_pg_oid(oid)),
        }
    }

    /// Returns the KingbaseES MySQL-compatible built-in type for an OID.
    pub fn from_mysql_oid(oid: Oid) -> Option<Type> {
        mysql_type_gen::Inner::from_oid(oid)
            .map(|inner| Type(Inner::Mysql(inner)))
            .or_else(|| Self::from_pg_oid(oid))
    }

    /// Returns the KingbaseES Oracle-compatible built-in type for an OID.
    pub fn from_oracle_oid(oid: Oid) -> Option<Type> {
        oracle_type_gen::Inner::from_oid(oid)
            .map(|inner| Type(Inner::Oracle(inner)))
            .or_else(|| Self::from_pg_oid(oid))
    }

    /// Returns the KingbaseES SQL Server-compatible built-in type for an OID.
    pub fn from_sqlserver_oid(oid: Oid) -> Option<Type> {
        sqlserver_type_gen::Inner::from_oid(oid)
            .map(|inner| Type(Inner::SqlServer(inner)))
            .or_else(|| Self::from_pg_oid(oid))
    }

    /// Returns the `Type` corresponding to the provided KingbaseES MySQL-compatible
    /// `Oid` if it corresponds to a built-in type in that compatibility mode.
    ///
    /// This intentionally does not affect [`Type::from_oid`], which remains scoped
    /// to PostgreSQL built-in types. PostgreSQL built-ins are shared by every
    /// compatibility mode and are therefore not duplicated in this extension table.
    pub fn from_kingbase_mysql_oid(oid: Oid) -> Option<Type> {
        mysql_type_gen::Inner::from_oid(oid).map(|inner| Type(Inner::Mysql(inner)))
    }

    /// Returns only a KingbaseES Oracle-compatible extension type for an OID.
    pub fn from_kingbase_oracle_oid(oid: Oid) -> Option<Type> {
        oracle_type_gen::Inner::from_oid(oid).map(|inner| Type(Inner::Oracle(inner)))
    }

    /// Returns the `Type` corresponding to the provided KingbaseES SQL Server-compatible
    /// `Oid` if it corresponds to a built-in type in that compatibility mode.
    pub fn from_kingbase_sqlserver_oid(oid: Oid) -> Option<Type> {
        sqlserver_type_gen::Inner::from_oid(oid).map(|inner| Type(Inner::SqlServer(inner)))
    }

    /// Returns the built-in type system this type belongs to, if known.
    pub fn type_system(&self) -> Option<TypeSystem> {
        match self.0 {
            Inner::Pg(_) => Some(TypeSystem::Pg),
            Inner::Mysql(_) => Some(TypeSystem::Mysql),
            Inner::Oracle(_) => Some(TypeSystem::Oracle),
            Inner::SqlServer(_) => Some(TypeSystem::SqlServer),
            Inner::Other(_) => None,
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

/// Represents the kind of a Postgres type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A simple type like `VARCHAR` or `INTEGER`.
    Simple,
    /// An enumerated type along with its variants.
    Enum(Vec<String>),
    /// A KingbaseES MySQL-compatible dynamic `ENUM` type along with its variants.
    MySqlEnum(Vec<String>),
    /// A KingbaseES MySQL-compatible dynamic `SET` type.
    ///
    /// The server represents a value as text that may contain multiple labels.
    /// Its catalog entry does not expose the declared label set through `pg_enum`.
    MySqlSet,
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
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB,
        ORACLE_BLOB,
        SQLSERVER_BINARY,
        SQLSERVER_VARBINARY
    );
}

impl<'a> FromSql<'a> for &'a [u8] {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<&'a [u8], Box<dyn Error + Sync + Send>> {
        Ok(types::bytea_from_sql(raw))
    }

    accepts!(
        BYTEA,
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB,
        ORACLE_BLOB,
        SQLSERVER_BINARY,
        SQLSERVER_VARBINARY
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
        matches!(
            *ty,
            Type::VARCHAR
                | Type::TEXT
                | Type::BPCHAR
                | Type::NAME
                | Type::UNKNOWN
                | Type::MYSQL_BPCHARBYTE
                | Type::MYSQL_VARCHARBYTE
                | Type::MYSQL_LONGTEXT
                | Type::MYSQL_MEDIUMTEXT
                | Type::MYSQL_TINYTEXT
                | Type::MYSQL_CLOB
                | Type::MYSQL_NCLOB
                | Type::ORACLE_UROWID
                | Type::ORACLE_CLOB
                | Type::ORACLE_NCLOB
                | Type::ORACLE_BPCHARBYTE
                | Type::ORACLE_VARCHARBYTE
        ) || matches!(ty.kind(), Kind::MySqlEnum(_) | Kind::MySqlSet)
            || ty == &Type::SQLSERVER_NVARCHAR
            || ty == &Type::SQLSERVER_NCHAR
            || ty == &Type::SQLSERVER_BPCHARBYTE
            || ty == &Type::SQLSERVER_VARCHARBYTE
            || ty == &Type::SQLSERVER_SYSNAME
            || ty == &Type::ORACLE_BFILE
            || ty == &Type::XML
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

fn numeric_to_i64(mut raw: &[u8]) -> Result<i64, Box<dyn Error + Sync + Send>> {
    const SIGN_POS: u16 = 0x0000;
    const SIGN_NEG: u16 = 0x4000;
    const SIGN_NAN: u16 = 0xC000;

    if raw.len() < 8 {
        return Err("invalid NUMERIC value".into());
    }

    let ndigits = raw.get_i16();
    let weight = raw.get_i16();
    let sign = raw.get_u16();
    let dscale = raw.get_i16();

    if ndigits < 0 || dscale < 0 || raw.len() != ndigits as usize * 2 {
        return Err("invalid NUMERIC value".into());
    }
    if sign == SIGN_NAN {
        return Err("NaN NUMERIC values cannot be decoded as i64".into());
    }
    if sign != SIGN_POS && sign != SIGN_NEG {
        return Err("invalid NUMERIC sign".into());
    }

    let mut digits = Vec::with_capacity(ndigits as usize);
    for _ in 0..ndigits {
        let digit = raw.get_i16();
        if !(0..10000).contains(&digit) {
            return Err("invalid NUMERIC digit".into());
        }
        digits.push(digit);
    }

    if digits.is_empty() {
        return Ok(0);
    }

    let integer_groups =
        usize::try_from((i32::from(weight) + 1).max(0)).map_err(|_| "invalid NUMERIC weight")?;
    if digits
        .get(integer_groups..)
        .unwrap_or_default()
        .iter()
        .any(|&digit| digit != 0)
    {
        return Err("NUMERIC value is not an integer".into());
    }

    let limit = if sign == SIGN_NEG {
        1_u64 << 63
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0_u64;
    for index in 0..integer_groups {
        let digit = digits.get(index).copied().unwrap_or(0) as u64;
        magnitude = magnitude
            .checked_mul(10_000)
            .and_then(|value| value.checked_add(digit))
            .filter(|&value| value <= limit)
            .ok_or("NUMERIC value out of range for i64")?;
    }

    if sign == SIGN_NEG {
        if magnitude == 1_u64 << 63 {
            Ok(i64::MIN)
        } else {
            Ok(-(magnitude as i64))
        }
    } else {
        Ok(magnitude as i64)
    }
}

simple_from!(bool, bool_from_sql, BOOL, SQLSERVER_SYS_BIT);
simple_from!(i8, char_from_sql, CHAR, MYSQL_TINYINT, ORACLE_TINYINT);
simple_from!(i16, int2_from_sql, INT2);
impl<'a> FromSql<'a> for i32 {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<i32, Box<dyn Error + Sync + Send>> {
        types::int4_from_sql(raw)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(
            *ty,
            Type::INT4
                | Type::MYSQL_INT1
                | Type::MYSQL_INT3
                | Type::MYSQL_MEDIUMINT
                | Type::MYSQL_MIDDLEINT
                | Type::MYSQL_YEAR
        )
    }
}
simple_from!(u32, oid_from_sql, OID, MYSQL_UINT4);
impl<'a> FromSql<'a> for u64 {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<u64, Box<dyn Error + Sync + Send>> {
        let bytes: [u8; 8] = raw
            .try_into()
            .map_err(|_| format!("invalid mysql uint8 length: {}", raw.len()))?;
        Ok(u64::from_be_bytes(bytes))
    }

    accepts!(MYSQL_UINT8);
}
impl<'a> FromSql<'a> for i64 {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<i64, Box<dyn Error + Sync + Send>> {
        if ty == &Type::NUMERIC {
            return numeric_to_i64(raw);
        }

        types::int8_from_sql(raw)
    }

    accepts!(INT8, NUMERIC);
}
simple_from!(f32, float4_from_sql, FLOAT4);
simple_from!(f64, float8_from_sql, FLOAT8);

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
        MYSQL_DATETIME,
        MYSQL_SYS_TIMESTAMP,
        ORACLE_SYS_DATE,
        SQLSERVER_DATETIME,
        SQLSERVER_SMALLDATETIME
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
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB,
        ORACLE_BLOB,
        SQLSERVER_BINARY,
        SQLSERVER_VARBINARY
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
        MYSQL_BINARY,
        MYSQL_VARBINARY,
        MYSQL_BLOB,
        MYSQL_LONGBLOB,
        MYSQL_MEDIUMBLOB,
        MYSQL_TINYBLOB,
        ORACLE_BLOB,
        SQLSERVER_BINARY,
        SQLSERVER_VARBINARY
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
        matches!(
            *ty,
            Type::VARCHAR
                | Type::TEXT
                | Type::BPCHAR
                | Type::NAME
                | Type::UNKNOWN
                | Type::MYSQL_BPCHARBYTE
                | Type::MYSQL_VARCHARBYTE
                | Type::MYSQL_LONGTEXT
                | Type::MYSQL_MEDIUMTEXT
                | Type::MYSQL_TINYTEXT
                | Type::MYSQL_CLOB
                | Type::MYSQL_NCLOB
                | Type::ORACLE_UROWID
                | Type::ORACLE_CLOB
                | Type::ORACLE_NCLOB
                | Type::ORACLE_BPCHARBYTE
                | Type::ORACLE_VARCHARBYTE
        ) || matches!(ty.kind(), Kind::MySqlEnum(_) | Kind::MySqlSet)
            || ty == &Type::SQLSERVER_NVARCHAR
            || ty == &Type::SQLSERVER_NCHAR
            || ty == &Type::SQLSERVER_BPCHARBYTE
            || ty == &Type::SQLSERVER_VARCHARBYTE
            || ty == &Type::SQLSERVER_SYSNAME
            || ty == &Type::ORACLE_BFILE
            || ty == &Type::XML
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

simple_to!(bool, bool_to_sql, BOOL, SQLSERVER_SYS_BIT);
simple_to!(i8, char_to_sql, CHAR, MYSQL_TINYINT, ORACLE_TINYINT);
simple_to!(i16, int2_to_sql, INT2);
impl ToSql for i32 {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        types::int4_to_sql(*self, w);
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(
            *ty,
            Type::INT4
                | Type::MYSQL_INT1
                | Type::MYSQL_INT3
                | Type::MYSQL_MEDIUMINT
                | Type::MYSQL_MIDDLEINT
                | Type::MYSQL_YEAR
        )
    }

    to_sql_checked!();
}
simple_to!(u32, oid_to_sql, OID, MYSQL_UINT4);
impl ToSql for u64 {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        w.put_u64(*self);
        Ok(IsNull::No)
    }

    accepts!(MYSQL_UINT8);

    to_sql_checked!();
}
simple_to!(i64, int8_to_sql, INT8);
simple_to!(f32, float4_to_sql, FLOAT4);
simple_to!(f64, float8_to_sql, FLOAT8);

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
        MYSQL_DATETIME,
        MYSQL_SYS_TIMESTAMP,
        ORACLE_SYS_DATE,
        SQLSERVER_DATETIME,
        SQLSERVER_SMALLDATETIME
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
