use crate::{FromSql, IsNull, ToSql, Type};
use bytes::{BufMut, BytesMut};
use serde_1::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json_1::Value;
use std::error::Error;
use std::fmt::Debug;
use std::io::Read;

mod lax;

/// A wrapper type to allow arbitrary `Serialize`/`Deserialize` types to convert to Postgres JSON values.
///
/// Kingbase Oracle lax JSON is normalized when strict JSON deserialization fails.
/// This fallback uses owned values; borrowed fields require strict JSON input.
/// It applies to the shared JSON type (OID 114); JSONB decoding stays strict.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Json<T>(pub T);

fn is_jsonb(ty: &Type) -> bool {
    matches!(*ty, Type::JSONB | Type::MYSQL_SYS_JSON | Type::MYSQL_JSON)
}

impl<T: Serialize> Serialize for Json<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Json<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

impl<'a, T> FromSql<'a> for Json<T>
where
    T: Deserialize<'a>,
{
    fn from_sql(ty: &Type, mut raw: &'a [u8]) -> Result<Json<T>, Box<dyn Error + Sync + Send>> {
        if is_jsonb(ty) {
            let mut b = [0; 1];
            raw.read_exact(&mut b)?;
            // We only support version 1 of the jsonb binary format
            if b[0] != 1 {
                return Err("unsupported JSONB encoding version".into());
            }
        }
        match serde_json_1::de::from_slice(raw) {
            Ok(value) => Ok(Json(value)),
            Err(error) => {
                // JSON (OID 114) is shared across database modes. Valid strict
                // JSON keeps the fast path, including borrowed deserialization.
                // Never apply Oracle text extensions to JSONB's wire format.
                if *ty == Type::JSON
                    && serde_json_1::from_slice::<serde_1::de::IgnoredAny>(raw).is_err()
                {
                    if let Some(normalized) = lax::normalize(raw) {
                        if let Ok(value) = serde_json_1::from_slice::<Value>(&normalized) {
                            return T::deserialize(value).map(Json).map_err(Into::into);
                        }
                    }
                }
                Err(error.into())
            }
        }
    }

    accepts!(JSON, JSONB, MYSQL_SYS_JSON, MYSQL_JSON);
}

impl<T> ToSql for Json<T>
where
    T: Serialize + Debug,
{
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        if is_jsonb(ty) {
            out.put_u8(1);
        }
        serde_json_1::ser::to_writer(out.writer(), &self.0)?;
        Ok(IsNull::No)
    }

    accepts!(JSON, JSONB, MYSQL_SYS_JSON, MYSQL_JSON);
    to_sql_checked!();
}

impl<'a> FromSql<'a> for Value {
    fn from_sql(ty: &Type, raw: &[u8]) -> Result<Value, Box<dyn Error + Sync + Send>> {
        Json::<Value>::from_sql(ty, raw).map(|json| json.0)
    }

    accepts!(JSON, JSONB, MYSQL_SYS_JSON, MYSQL_JSON);
}

impl ToSql for Value {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        Json(self).to_sql(ty, out)
    }

    accepts!(JSON, JSONB, MYSQL_SYS_JSON, MYSQL_JSON);
    to_sql_checked!();
}
