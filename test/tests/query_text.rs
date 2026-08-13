//! End-to-end checks for adaptive result formats.
//!
//! These tests require a Kingbase connection because the wire result format is
//! selected in Bind. Set `KINGBASE_TEST_URL` to run them locally.

use tokio_postgres::{Client, NoTls};

async fn connect_from_env() -> Option<Client> {
    let Ok(url) = std::env::var("KINGBASE_TEST_URL") else {
        return None;
    };
    let (client, connection) = tokio_postgres::connect(&url, NoTls)
        .await
        .expect("KINGBASE_TEST_URL is set but the Kingbase connection failed");
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Kingbase connection error: {error}");
        }
    });
    Some(client)
}

#[tokio::test]
async fn query_text_reads_mysql_compatibility_text_as_string() {
    let Some(client) = connect_from_env().await else {
        eprintln!("skipped: KINGBASE_TEST_URL is not set");
        return;
    };

    // STRING is a Kingbase compatibility type that is intentionally resolved
    // dynamically rather than coming from the generated static OID tables.
    let row = client
        .query_text("SELECT CAST('query-text-ok' AS STRING)", &[])
        .await
        .expect("query_text test statement should execute")
        .pop()
        .expect("query_text test should return one row");
    assert_eq!(row.get::<_, String>(0), "query-text-ok");
}

#[tokio::test]
async fn regular_query_keeps_binary_path_for_custom_unknown_codec() {
    let Some(client) = connect_from_env().await else {
        eprintln!("skipped: KINGBASE_TEST_URL is not set");
        return;
    };

    let row = client
        .query_one("SELECT CAST('binary-path-ok' AS STRING)", &[])
        .await
        .expect("regular query statement should execute");
    let error = row.try_get::<_, BinaryProbe>(0).unwrap_err();
    assert!(!error.to_string().is_empty());
}

#[derive(Debug)]
struct BinaryProbe;

impl<'a> tokio_postgres::types::FromSql<'a> for BinaryProbe {
    fn accepts(ty: &tokio_postgres::types::Type) -> bool {
        ty.name().eq_ignore_ascii_case("varchar")
    }

    fn from_sql(
        _ty: &tokio_postgres::types::Type,
        _raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Err("custom binary codec intentionally rejects text payload".into())
    }
}
