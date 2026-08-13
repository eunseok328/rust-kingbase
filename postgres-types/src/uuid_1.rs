use bytes::BytesMut;
use postgres_protocol::types;
use std::error::Error;
use uuid_1::Uuid;

use crate::{FromSql, IsNull, ToSql, Type};

impl<'a> FromSql<'a> for Uuid {
    fn from_sql(_: &Type, raw: &[u8]) -> Result<Uuid, Box<dyn Error + Sync + Send>> {
        let bytes = types::uuid_from_sql(raw)?;
        Ok(Uuid::from_bytes(bytes))
    }

    accepts!(UUID, SQLSERVER_UNIQUEIDENTIFIER);
}

impl ToSql for Uuid {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        types::uuid_to_sql(*self.as_bytes(), w);
        Ok(IsNull::No)
    }

    accepts!(UUID, SQLSERVER_UNIQUEIDENTIFIER);
    to_sql_checked!();
}

#[cfg(test)]
mod tests {
    use super::Uuid;
    use crate::{FromSql, ToSql, Type};

    #[test]
    fn uuid_reuses_sqlserver_uniqueidentifier_wire_format() {
        let value = Uuid::from_bytes([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);

        assert!(<Uuid as FromSql>::accepts(
            &Type::SQLSERVER_UNIQUEIDENTIFIER
        ));
        assert!(<Uuid as ToSql>::accepts(&Type::SQLSERVER_UNIQUEIDENTIFIER));
        assert!(<Vec<Uuid> as FromSql<'_>>::accepts(
            &Type::SQLSERVER_UNIQUEIDENTIFIER_ARRAY
        ));
        assert!(<Vec<Uuid> as ToSql>::accepts(
            &Type::SQLSERVER_UNIQUEIDENTIFIER_ARRAY
        ));
        assert!(!<Uuid as FromSql>::accepts(&Type::SQLSERVER_NVARCHAR));

        let decoded =
            <Uuid as FromSql>::from_sql(&Type::SQLSERVER_UNIQUEIDENTIFIER, value.as_bytes())
                .unwrap();
        assert_eq!(decoded, value);
    }
}
