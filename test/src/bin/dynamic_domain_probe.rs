use tokio_postgres::NoTls;

const CONNECTION: &str =
    "host=127.0.0.1 port=54322 user=system password=0319 dbname=test";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (client, connection) = tokio_postgres::connect(CONNECTION, NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Kingbase connection error: {error}");
        }
    });

    // BINARY_INTEGER is visible in the shared Kingbase catalog but is not in
    // a generated mode table. ParameterDescription therefore exercises the
    // dynamic pg_type -> typbasetype -> INT4 path.
    let statement = client
        .prepare("SELECT CAST(? AS BINARY_INTEGER) AS value")
        .await?;
    let parameter = &statement.params()[0];
    assert_eq!(parameter.name(), "BINARY_INTEGER");
    assert_eq!(parameter.type_system(), None);

    let row = client.query_one(&statement, &[&42_i32]).await?;
    assert_eq!(row.get::<_, i32>(0), 42);

    let text_statement = client
        .prepare("SELECT CAST(? AS STRING) AS value")
        .await?;
    let text_parameter = &text_statement.params()[0];
    assert_eq!(text_parameter.name(), "STRING");
    assert_eq!(text_parameter.type_system(), None);

    let text_row = client
        .query_one(&text_statement, &[&"domain text"])
        .await?;
    assert_eq!(text_row.get::<_, String>(0), "domain text");

    let domain_oid = parameter.oid();
    let int_type = client
        .query_one(
            "SELECT t.typname, b.typname FROM pg_type t \
             JOIN pg_type b ON b.oid = t.typbasetype \
             WHERE t.oid = ?",
            &[&domain_oid],
        )
        .await?;
    println!(
        "dynamic domain decoded: {} over {}",
        int_type.get::<_, String>(0),
        int_type.get::<_, String>(1)
    );
    println!(
        "dynamic domain decoded: {} (oid={})",
        text_parameter.name(),
        text_parameter.oid()
    );

    Ok(())
}
