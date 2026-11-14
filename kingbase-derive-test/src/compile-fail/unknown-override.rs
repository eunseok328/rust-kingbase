use kingbase_types::{FromSql, ToSql};

#[derive(FromSql)]
#[kingbase(foo = "bar")]
struct Foo {
    a: i32,
}

#[derive(ToSql)]
#[kingbase(foo = "bar")]
struct Bar {
    a: i32,
}

fn main() {}
