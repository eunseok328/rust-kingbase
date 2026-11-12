use kingbase_types::{FromSql, ToSql};

#[derive(ToSql, Debug)]
#[kingbase(transparent)]
struct ToSqlTransparentStruct {
    a: i32
}

#[derive(FromSql, Debug)]
#[kingbase(transparent)]
struct FromSqlTransparentStruct {
    a: i32
}

#[derive(ToSql, Debug)]
#[kingbase(transparent)]
enum ToSqlTransparentEnum {
    Foo
}

#[derive(FromSql, Debug)]
#[kingbase(transparent)]
enum FromSqlTransparentEnum {
    Foo
}

#[derive(ToSql, Debug)]
#[kingbase(transparent)]
struct ToSqlTransparentTwoFieldTupleStruct(i32, i32);

#[derive(FromSql, Debug)]
#[kingbase(transparent)]
struct FromSqlTransparentTwoFieldTupleStruct(i32, i32);

fn main() {}
