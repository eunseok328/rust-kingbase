use chrono::NaiveDate;
use postgres_types::Type;
use std::env;
use tokio_postgres::{CompatibleMode, NoTls};

async fn connect() -> tokio_postgres::Client {
    let url = env::var("KINGBASE_TEST_URL")
        .or_else(|_| env::var("KINGBASE_MYSQL_URL"))
        .expect("set KINGBASE_TEST_URL or KINGBASE_MYSQL_URL");
    let (client, connection) = tokio_postgres::connect(&url, NoTls)
        .await
        .expect("connect to Kingbase MySQL mode");
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Kingbase connection broken: {}", e);
        }
    });
    client
}

#[tokio::test]
async fn mysql_sys_date_reuse_pg_date_codec_with_naive_date() {
    let client = connect().await;
    assert_eq!(client.compatible_mode(), CompatibleMode::Mysql);

    // 建表
    client
        .batch_execute(
            "DROP TABLE IF EXISTS rust_kingbase_sys_date_probe;
             CREATE TABLE rust_kingbase_sys_date_probe (
                id INT,
                event_date DATE,
                int1_value int1
            )",
        )
        .await
        .unwrap();

    let value = NaiveDate::from_ymd_opt(2020, 4, 20).unwrap();

    // ========== 写入侧：显式 prepare，校验参数期望类型 ==========
    let insert_stmt = client
        .prepare(
            "INSERT INTO rust_kingbase_sys_date_probe(id, event_date, int1_value) VALUES (?, ?, ?)",
        )
        .await
        .unwrap();

    // 断言：第二个参数数据库期望类型是 MYSQL_SYS_DATE
    assert_eq!(insert_stmt.params()[1], Type::MYSQL_SYS_DATE);
    assert_eq!(insert_stmt.params()[2], Type::MYSQL_INT1);

    // 使用 NaiveDate 和 i32 写入
    client
        .execute(&insert_stmt, &[&1_i32, &value, &127_i32])
        .await
        .unwrap();

    // ========== 查询侧：校验结果列类型并解码 ==========
    let select_stmt = client
        .prepare(
            "SELECT id, event_date, int1_value FROM rust_kingbase_sys_date_probe WHERE id = ?",
        )
        .await
        .unwrap();

    // 结果列 event_date 类型校验
    assert_eq!(select_stmt.columns()[1].type_(), &Type::MYSQL_SYS_DATE);
    // Kingbase RowDescription returns the int1 domain's base INT4 type.
    assert_eq!(select_stmt.columns()[2].type_(), &Type::INT4);

    let row = client.query_one(&select_stmt, &[&1_i32]).await.unwrap();
    assert_eq!(row.get::<_, i32>(0), 1);
    assert_eq!(row.get::<_, NaiveDate>(1), value);
    assert_eq!(row.get::<_, i32>(2), 127);
}
