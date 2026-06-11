use crate::client::CompatibleMode;
use crate::types::{Oid, Type, TypeSystem};

pub(crate) fn type_from_oid(type_system: TypeSystem, oid: Oid) -> Option<Type> {
    Type::from_oid_in(type_system, oid)
}

pub(crate) fn param_codec_type(_mode: CompatibleMode, _type_: &Type) -> Option<Type> {
    None
}

pub(crate) fn read_codec_type(_mode: CompatibleMode, _type_: &Type) -> Option<Type> {
    None
}

#[cfg(test)]
mod tests {
    use super::{param_codec_type, read_codec_type, type_from_oid};
    use crate::client::CompatibleMode;
    use crate::types::{Type, TypeSystem};

    #[test]
    fn mysql_mode_uses_mysql_type_table_without_codec_aliasing() {
        let mysql_int4 = type_from_oid(TypeSystem::Mysql, 23).unwrap();
        assert_ne!(mysql_int4, Type::INT4);
        assert_eq!(mysql_int4.oid(), Type::INT4.oid());
        assert_eq!(param_codec_type(CompatibleMode::Mysql, &mysql_int4), None);
        assert_eq!(read_codec_type(CompatibleMode::Mysql, &mysql_int4), None);
    }

    #[test]
    fn pg_mode_does_not_apply_mysql_codec_policy() {
        let pg_int4 = type_from_oid(TypeSystem::Pg, 23).unwrap();
        assert_eq!(pg_int4, Type::INT4);
        assert_eq!(param_codec_type(CompatibleMode::Pg, &pg_int4), None);
        assert_eq!(read_codec_type(CompatibleMode::Pg, &pg_int4), None);
    }
}
