#![cfg(feature = "runtime")]

use std::collections::HashMap;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, CompatibleMode, NoTls};

// Exercise the private lexer without exposing a new public driver API.
mod client {
    pub use tokio_postgres::CompatibleMode;
}
#[path = "../src/sql_compat.rs"]
mod sql_compat;

use sql_compat::{SqlParseSettings, rewrite_query};

fn rewrite(mode: CompatibleMode, sql: &str) -> String {
    rewrite_query(mode, SqlParseSettings::default(), sql)
        .unwrap()
        .into_owned()
}

#[test]
fn mysql_protects_quotes_comments_and_explicit_slots() {
    let sql = "SELECT $1, ?, '?', \u{60}?\u{60}, $$?$$, $tag$?$tag$, ?? /* ? /* ? */ ? */ -- ?\n, ? # ?\n";
    assert_eq!(
        rewrite(CompatibleMode::Mysql, sql),
        "SELECT $1, $2, '?', \u{60}?\u{60}, $$?$$, $tag$?$tag$, ? /* ? /* ? */ ? */ -- ?\n, $3 # ?\n"
    );
    assert_eq!(
        rewrite(CompatibleMode::Mysql, "SELECT $2, ?"),
        "SELECT $2, $1"
    );
}

#[test]
fn strings_follow_startup_settings_and_explicit_escape_prefixes() {
    let settings = SqlParseSettings::from_parameters(&HashMap::from([(
        "standard_conforming_strings".to_string(),
        "off".to_string(),
    )]));
    assert_eq!(
        rewrite_query(CompatibleMode::Mysql, settings, r"SELECT 'a\'?b', ?").unwrap(),
        r"SELECT 'a\'?b', $1"
    );
    for (mode, parameter) in [
        (CompatibleMode::Mysql, "?"),
        (CompatibleMode::Oracle, ":id"),
        (CompatibleMode::SqlServer, "@id"),
    ] {
        let sql = format!(r"SELECT '\', {parameter}");
        assert_eq!(rewrite(mode, &sql), r"SELECT '\', $1");
        let sql = format!(r"SELECT E'a\'? :hidden @hidden', {parameter}");
        assert_eq!(rewrite(mode, &sql), r"SELECT E'a\'? :hidden @hidden', $1");
    }
}

#[test]
fn oracle_binds_every_occurrence_including_numeric_labels() {
    assert_eq!(
        rewrite(
            CompatibleMode::Oracle,
            "SELECT :id, :id, :2, :1, :0 FROM dual"
        ),
        "SELECT $1, $2, $3, $4, $5 FROM dual"
    );
    assert_eq!(
        rewrite(CompatibleMode::Oracle, "SELECT $2, :id, :id FROM dual"),
        "SELECT $2, $1, $3 FROM dual"
    );
}

#[test]
fn oracle_anonymous_blocks_and_calls_reuse_parameter_names() {
    for (sql, expected) in [
        (
            "BEGIN p(:x, :X, :y, :x); END;",
            "BEGIN p($1, $1, $2, $1); END;",
        ),
        (
            "DECLARE value INT := :2; BEGIN p(value, :1, :2); END;",
            "DECLARE value INT := $1; BEGIN p(value, $2, $1); END;",
        ),
        (
            "-- CALL :ignored\n /* :ignored */ cAlL p(:x, :X, :y, :x)",
            "-- CALL :ignored\n /* :ignored */ cAlL p($1, $1, $2, $1)",
        ),
        (
            "<<outer_block>> DECLARE value INT := :x; BEGIN p(value, :X); END;",
            "<<outer_block>> DECLARE value INT := $1; BEGIN p(value, $1); END;",
        ),
    ] {
        assert_eq!(rewrite(CompatibleMode::Oracle, sql), expected);
    }
}

#[test]
fn oracle_name_reuse_preserves_explicit_slots_and_dynamic_sql() {
    assert_eq!(
        rewrite(CompatibleMode::Oracle, "CALL p($2, :x, :X, :y, :2, :2)"),
        "CALL p($2, $1, $1, $3, $4, $4)"
    );
    let sql = "BEGIN EXECUTE IMMEDIATE nq'[BEGIN p(:x, :x, :y); END;]' USING :value, :VALUE; END;";
    assert_eq!(
        rewrite(CompatibleMode::Oracle, sql),
        "BEGIN EXECUTE IMMEDIATE nq'[BEGIN p(:x, :x, :y); END;]' USING $1, $1; END;"
    );
    assert_eq!(
        rewrite(CompatibleMode::Oracle, "CALL p(:Ä, :ä, :Ä)"),
        "CALL p($1, $2, $1)"
    );
}

#[test]
fn oracle_ordinary_sql_keeps_positional_binding() {
    for sql in [
        "/* BEGIN CALL */ SELECT :x, :x FROM dual",
        "SELECT 'CALL', q'[BEGIN :hidden]', :x, :X FROM dual",
        "DECLARE c CURSOR FOR SELECT :x, :x FROM dual",
    ] {
        let rewritten = rewrite(CompatibleMode::Oracle, sql);
        assert!(rewritten.contains("$1, $2"), "{rewritten}");
    }
}

#[test]
fn block_comments_follow_initial_setting() {
    let settings = SqlParseSettings::from_parameters(&HashMap::from([(
        "enable_unpaired_comment".to_string(),
        "on".to_string(),
    )]));
    for (mode, first, second) in [
        (CompatibleMode::Mysql, "?", "?"),
        (CompatibleMode::Oracle, ":alpha", ":beta"),
    ] {
        let sql = format!("SELECT {first}, /* outer /* hidden */ {second}");
        assert_eq!(
            rewrite_query(mode, settings, &sql).unwrap(),
            "SELECT $1, /* outer /* hidden */ $2"
        );
    }
}

#[test]
fn block_comments_follow_setting_updates() {
    let mut settings = SqlParseSettings::default();
    let sql = "SELECT /* outer /* hidden */ $1, ?";
    assert_eq!(
        rewrite_query(CompatibleMode::Mysql, settings, sql).unwrap(),
        sql
    );
    settings.update("enable_unpaired_comment", "ON");
    assert_eq!(
        rewrite_query(CompatibleMode::Mysql, settings, sql).unwrap(),
        "SELECT /* outer /* hidden */ $1, $2"
    );
    settings.update("enable_unpaired_comment", "off");
    assert_eq!(
        rewrite_query(CompatibleMode::Mysql, settings, sql).unwrap(),
        sql
    );
}

#[test]
fn block_comments_missing_setting_keep_nesting() {
    let settings = SqlParseSettings::from_parameters(&HashMap::new());
    let sql = "SELECT /* outer /* @hidden */ @hidden */ @value";
    assert_eq!(
        rewrite_query(CompatibleMode::SqlServer, settings, sql).unwrap(),
        "SELECT /* outer /* @hidden */ @hidden */ $1"
    );
}

#[test]
fn block_comments_non_nested_openers_match_kes_lexer() {
    let settings = SqlParseSettings::from_parameters(&HashMap::from([(
        "enable_unpaired_comment".to_string(),
        "on".to_string(),
    )]));
    for (mode, parameter) in [
        (CompatibleMode::Mysql, "?"),
        (CompatibleMode::Oracle, ":value"),
    ] {
        let sql = format!("SELECT /*/*/ {parameter} */ {parameter}");
        assert_eq!(
            rewrite_query(mode, settings, &sql).unwrap(),
            format!("SELECT /*/*/ {parameter} */ $1")
        );
    }
}

#[test]
fn oracle_protects_alternative_literals_identifiers_and_trigger_records() {
    let sql = "SELECT nq'[don't bind :hidden ?]', NQ'{:hidden}', q'界:fake界', col#x, col$id, :a$b, :c#d, :中文, :NEW.id, :OLD.id, :=, value::text /* :fake /* :fake */ :fake */ FROM dual -- :fake\n";
    assert_eq!(
        rewrite(CompatibleMode::Oracle, sql),
        "SELECT nq'[don't bind :hidden ?]', NQ'{:hidden}', q'界:fake界', col#x, col$id, $1, $2, $3, :NEW.id, :OLD.id, :=, value::text /* :fake /* :fake */ :fake */ FROM dual -- :fake\n"
    );
    assert_eq!(
        rewrite(CompatibleMode::Oracle, "SELECT :new, :old FROM dual"),
        "SELECT $1, $2 FROM dual"
    );
}

#[test]
fn oracle_and_sqlserver_do_not_rewrite_question_marks() {
    for mode in [CompatibleMode::Oracle, CompatibleMode::SqlServer] {
        let sql = "SELECT ?, ??, '?' /* ? */";
        assert_eq!(rewrite(mode, sql), sql);
    }
}

#[test]
fn sqlserver_names_are_complete_and_reuse_slots() {
    assert_eq!(
        rewrite(
            CompatibleMode::SqlServer,
            "SELECT @id, @ID, @a$b, @a#b, @中文, $2"
        ),
        "SELECT $1, $1, $3, $4, $5, $2"
    );
    assert_eq!(
        rewrite(
            CompatibleMode::SqlServer,
            "SELECT column@name FROM t WHERE id = @id"
        ),
        "SELECT column@name FROM t WHERE id = $1"
    );
}

#[test]
fn sqlserver_preserves_local_variables_procedure_labels_and_system_variables() {
    assert_eq!(
        rewrite(
            CompatibleMode::SqlServer,
            "DECLARE @Local INT = @seed, @other DECIMAL(10,2) = @seed; SELECT @local, @OTHER, @id"
        ),
        "DECLARE @Local INT = $1, @other DECIMAL(10,2) = $1; SELECT @local, @OTHER, $2"
    );
    assert_eq!(
        rewrite(
            CompatibleMode::SqlServer,
            "EXEC dbo.proc @arg = @value, @other = @VALUE"
        ),
        "EXEC dbo.proc @arg = $1, @other = $1"
    );
    let sql = "SELECT '@id', [@id], [a]]@id], \"@id\", @@ROWCOUNT, @id /* @fake /* @fake */ @fake */ FROM #tmp";
    assert_eq!(
        rewrite(CompatibleMode::SqlServer, sql),
        "SELECT '@id', [@id], [a]]@id], \"@id\", @@ROWCOUNT, $1 /* @fake /* @fake */ @fake */ FROM #tmp"
    );
}

#[test]
fn sqlserver_rejects_ambiguous_assignments_and_module_definitions() {
    for sql in [
        "SET @unknown = @value",
        "SELECT @unknown = @value",
        "CREATE PROCEDURE p @arg INT AS SELECT @arg",
        "CREATE OR ALTER FUNCTION f(@arg INT) RETURNS INT AS SELECT @arg",
    ] {
        assert!(
            rewrite_query(CompatibleMode::SqlServer, SqlParseSettings::default(), sql).is_err()
        );
    }
}

#[test]
fn pg_queries_are_borrowed_without_changes() {
    let sql = "SELECT $2, $1, ?, ??, :name, @name";
    assert!(matches!(
        rewrite_query(CompatibleMode::Pg, SqlParseSettings::default(), sql).unwrap(),
        std::borrow::Cow::Borrowed(s) if s == sql
    ));
}

async fn connect(variable: &str, mode: CompatibleMode) -> Client {
    let config = std::env::var(variable)
        .unwrap_or_else(|_| panic!("set {variable} to run KingbaseES integration tests"));
    let (client, connection) = tokio_postgres::connect(&config, NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    assert_eq!(client.compatible_mode(), mode);
    client
        .batch_execute("SET statement_timeout = '5s'")
        .await
        .unwrap();
    client
}

#[tokio::test]
#[ignore = "requires KINGBASE_MYSQL_URL"]
async fn mysql_live_quotes_comments_explicit_slots_and_statement_reuse() {
    let client = connect("KINGBASE_MYSQL_URL", CompatibleMode::Mysql).await;
    let statement = client
        .prepare("SELECT '?' AS literal, /* outer /* ? */ ? */ CAST(? AS INT) AS bound")
        .await
        .unwrap();
    assert_eq!(statement.params(), &[Type::INT4]);
    for value in [7_i32, 11] {
        let row = client.query_one(&statement, &[&value]).await.unwrap();
        assert_eq!(row.get::<_, String>(0), "?");
        assert_eq!(row.get::<_, i32>(1), value);
    }
    let row = client
        .query_one("SELECT CAST($1 AS INT), CAST(? AS INT)", &[&7_i32, &11_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    let rows = client
        .query_typed(
            "SELECT '?' AS literal, CAST(? AS INT)",
            &[(&7_i32, Type::INT4)],
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get::<_, i32>(1), 7);
}

#[tokio::test]
#[ignore = "requires KINGBASE_MYSQL_URL"]
async fn mysql_live_setting_changes_and_transaction_rollback_are_connection_local() {
    let mut client = connect("KINGBASE_MYSQL_URL", CompatibleMode::Mysql).await;
    let other = connect("KINGBASE_MYSQL_URL", CompatibleMode::Mysql).await;
    client
        .batch_execute("SET standard_conforming_strings = on")
        .await
        .unwrap();
    other
        .batch_execute("SET standard_conforming_strings = on")
        .await
        .unwrap();

    let transaction = client.transaction().await.unwrap();
    transaction
        .batch_execute("SET LOCAL standard_conforming_strings = off")
        .await
        .unwrap();
    let row = transaction
        .query_one(r"SELECT 'a\'?b' AS literal, CAST(? AS INT)", &[&7_i32])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "a'?b");
    assert_eq!(row.get::<_, i32>(1), 7);

    let row = other
        .query_one(r"SELECT '\' AS literal, CAST(? AS INT)", &[&11_i32])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "\\");
    assert_eq!(row.get::<_, i32>(1), 11);
    transaction.rollback().await.unwrap();

    let row = client
        .query_one(r"SELECT '\' AS literal, CAST(? AS INT)", &[&7_i32])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "\\");
    assert_eq!(row.get::<_, i32>(1), 7);
    let rows = client
        .query_typed(
            r"SELECT E'a\'?b' AS literal, CAST(? AS INT)",
            &[(&7_i32, Type::INT4)],
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get::<_, String>(0), "a'?b");
    assert_eq!(rows[0].get::<_, i32>(1), 7);
}

#[tokio::test]
#[ignore = "requires KINGBASE_ORACLE_URL"]
async fn oracle_live_positions_alternative_literals_and_parameter_names() {
    let client = connect("KINGBASE_ORACLE_URL", CompatibleMode::Oracle).await;
    let row = client
        .query_one(
            "SELECT nq'[don't bind :hidden ?]' AS literal, col#x, CAST(:value AS INT), CAST(:value AS INT), CAST(:a$b AS INT), CAST(:c#d AS INT), CAST(:中文 AS INT) FROM (SELECT 1 AS col#x FROM dual) t",
            &[&7_i32, &11_i32, &13_i32, &17_i32, &19_i32],
        ).await.unwrap();
    assert_eq!(row.get::<_, String>(0), "don't bind :hidden ?");
    for (column, value) in [1, 7, 11, 13, 17, 19].into_iter().enumerate() {
        assert_eq!(row.get::<_, i32>(column + 1), value);
    }
    let row = client
        .query_one(
            "SELECT CAST(:2 AS INT), CAST(:1 AS INT) FROM dual",
            &[&7_i32, &11_i32],
        )
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    let row = client
        .query_one(
            "SELECT CAST(:alpha AS INT), col#x, CAST(:beta AS INT) FROM (SELECT 1 AS col#x FROM dual) t /* outer /* :fake */ :fake */",
            &[&7_i32, &11_i32],
        ).await.unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(2)), (7, 11));
}

#[tokio::test]
#[ignore = "requires KINGBASE_ORACLE_URL"]
async fn oracle_live_anonymous_blocks_and_calls_reuse_parameters() {
    let mut client = connect("KINGBASE_ORACLE_URL", CompatibleMode::Oracle).await;
    client
        .batch_execute(
            "CREATE TEMP TABLE placeholder_bind_results(a INT, b INT, c INT, d INT);
             CREATE PROCEDURE pg_temp.placeholder_bind_record(IN a INT, IN b INT, IN c INT, IN d INT)
             AS BEGIN INSERT INTO pg_temp.placeholder_bind_results VALUES (a, b, c, d); END;",
        )
        .await
        .unwrap();

    let call = "/* :ignored */ CALL pg_temp.placeholder_bind_record(:x, :X, :y, :x)";
    let statement = client
        .prepare_typed(call, &[Type::INT4, Type::INT4])
        .await
        .unwrap();
    assert_eq!(statement.params(), &[Type::INT4, Type::INT4]);
    for (x, y) in [(7_i32, 11_i32), (13, 17)] {
        client.execute(&statement, &[&x, &y]).await.unwrap();
    }
    assert!(
        client
            .query_typed(call, &[(&19_i32, Type::INT4), (&23_i32, Type::INT4)])
            .await
            .unwrap()
            .is_empty()
    );
    client
        .execute_typed(call, &[(&29_i32, Type::INT4), (&31_i32, Type::INT4)])
        .await
        .unwrap();

    let block = "DECLARE local_value INT := :x; BEGIN
        INSERT INTO pg_temp.placeholder_bind_results VALUES (local_value, :X, :y, :x);
        END;";
    let block_statement = client
        .prepare_typed(block, &[Type::INT4, Type::INT4])
        .await
        .unwrap();
    assert_eq!(block_statement.params(), &[Type::INT4, Type::INT4]);
    client
        .execute(&block_statement, &[&37_i32, &41_i32])
        .await
        .unwrap();
    client
        .execute_typed(
            "BEGIN
             INSERT INTO pg_temp.placeholder_bind_results VALUES (:2, :2, :1, :2);
             END;",
            &[(&43_i32, Type::INT4), (&47_i32, Type::INT4)],
        )
        .await
        .unwrap();
    client
        .execute_typed(
            "BEGIN INSERT INTO pg_temp.placeholder_bind_results VALUES ($2, :x, :X, :y); END;",
            &[
                (&53_i32, Type::INT4),
                (&59_i32, Type::INT4),
                (&61_i32, Type::INT4),
            ],
        )
        .await
        .unwrap();

    // The inner SQL string remains positional and is interpreted by the server;
    // only the outer USING arguments are rewritten and reuse bind slots.
    client
        .execute_typed(
            "DECLARE result_value INT; BEGIN
             EXECUTE IMMEDIATE 'SELECT CAST(:arg AS INT) + CAST(:arg AS INT) FROM dual'
                 INTO result_value USING :value, :VALUE;
             INSERT INTO pg_temp.placeholder_bind_results
                 VALUES (result_value, :value, :other, :OTHER);
             END;",
            &[(&67_i32, Type::INT4), (&73_i32, Type::INT4)],
        )
        .await
        .unwrap();

    let transaction = client.transaction().await.unwrap();
    transaction
        .execute(&statement, &[&79_i32, &83_i32])
        .await
        .unwrap();
    transaction.rollback().await.unwrap();

    let rows = client
        .query(
            "SELECT a, b, c, d FROM pg_temp.placeholder_bind_results ORDER BY a",
            &[],
        )
        .await
        .unwrap();
    let values = rows
        .iter()
        .map(|row| {
            (
                row.get::<_, i32>(0),
                row.get::<_, i32>(1),
                row.get::<_, i32>(2),
                row.get::<_, i32>(3),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            (7, 7, 11, 7),
            (13, 13, 17, 13),
            (19, 19, 23, 19),
            (29, 29, 31, 29),
            (37, 37, 41, 37),
            (43, 43, 47, 43),
            (59, 53, 53, 61),
            (134, 67, 73, 73),
        ]
    );
    client.check_connection().await.unwrap();
}

#[tokio::test]
#[ignore = "requires KINGBASE_ORACLE_URL"]
async fn oracle_live_prepared_typed_transaction_and_no_question_binds() {
    let mut client = connect("KINGBASE_ORACLE_URL", CompatibleMode::Oracle).await;
    let stmt = client
        .prepare_typed(
            "SELECT CAST(:id AS INT), CAST(:id AS INT) FROM dual",
            &[Type::INT4, Type::INT4],
        )
        .await
        .unwrap();
    assert_eq!(stmt.params().len(), 2);
    for (a, b) in [(7_i32, 11_i32), (13, 17)] {
        let row = client.query_one(&stmt, &[&a, &b]).await.unwrap();
        assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (a, b));
    }
    let rows = client
        .query_typed(
            "SELECT CAST(:id AS INT), CAST(:id AS INT) FROM dual",
            &[(&7_i32, Type::INT4), (&11_i32, Type::INT4)],
        )
        .await
        .unwrap();
    assert_eq!(
        (rows[0].get::<_, i32>(0), rows[0].get::<_, i32>(1)),
        (7, 11)
    );
    assert_eq!(
        client
            .execute_typed(
                "SELECT CAST(:value AS INT) FROM dual",
                &[(&7_i32, Type::INT4)]
            )
            .await
            .unwrap(),
        1
    );
    let tx = client.transaction().await.unwrap();
    let row = tx
        .query_one("SELECT CAST(:value AS INT) FROM dual", &[&7_i32])
        .await
        .unwrap();
    assert_eq!(row.get::<_, i32>(0), 7);
    tx.rollback().await.unwrap();
    let error = client
        .prepare("SELECT CAST(? AS INT) FROM dual")
        .await
        .unwrap_err();
    assert_eq!(error.code().unwrap().code(), "42601");
    client.check_connection().await.unwrap();
}

#[tokio::test]
#[ignore = "requires KINGBASE_SQLSERVER_URL"]
async fn sqlserver_live_names_case_literals_and_no_question_binds() {
    let client = connect("KINGBASE_SQLSERVER_URL", CompatibleMode::SqlServer).await;
    let stmt = client
        .prepare("SELECT '@id ?' AS literal, CAST(@id AS INT), CAST(@ID AS INT), CAST(@a$b AS INT), CAST(@a#b AS INT), CAST(@中文 AS INT)")
        .await.unwrap();
    assert_eq!(stmt.params().len(), 4);
    let row = client
        .query_one(&stmt, &[&7_i32, &11_i32, &13_i32, &17_i32])
        .await
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "@id ?");
    for (column, value) in [7, 7, 11, 13, 17].into_iter().enumerate() {
        assert_eq!(row.get::<_, i32>(column + 1), value);
    }
    let rows = client
        .query_typed(
            "SELECT CAST(@id AS INT), CAST(@ID AS INT)",
            &[(&7_i32, Type::INT4)],
        )
        .await
        .unwrap();
    assert_eq!((rows[0].get::<_, i32>(0), rows[0].get::<_, i32>(1)), (7, 7));
    assert_eq!(
        client
            .execute_typed("SELECT CAST(@id AS INT)", &[(&7_i32, Type::INT4)])
            .await
            .unwrap(),
        1
    );
    let error = client.prepare("SELECT CAST(? AS INT)").await.unwrap_err();
    assert_eq!(error.code().unwrap().code(), "42601");
    client.check_connection().await.unwrap();
}

#[tokio::test]
#[ignore = "requires KINGBASE_PG_URL (PostgreSQL or KingbaseES PG mode)"]
async fn pg_live_explicit_slots_are_unchanged() {
    let client = connect("KINGBASE_PG_URL", CompatibleMode::Pg).await;
    let row = client
        .query_one(
            "SELECT CAST($2 AS INT), CAST($1 AS INT)",
            &[&7_i32, &11_i32],
        )
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (11, 7));
}

async fn verify_unpaired_comment_settings(variable: &str, mode: CompatibleMode, parameter: &str) {
    let mut config = std::env::var(variable)
        .unwrap_or_else(|_| panic!("set {variable} to run KingbaseES integration tests"))
        .parse::<tokio_postgres::Config>()
        .unwrap();
    config.options("-c enable_unpaired_comment=on");
    let (mut client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    assert_eq!(client.compatible_mode(), mode);
    assert_eq!(
        client
            .kingbase_server_setting("enable_unpaired_comment")
            .as_deref(),
        Some("on")
    );
    let other = connect(variable, mode).await;
    other
        .batch_execute("SET enable_unpaired_comment = off")
        .await
        .unwrap();
    let suffix = if mode == CompatibleMode::Oracle {
        " FROM dual"
    } else {
        ""
    };
    let on_sql = format!(
        "SELECT CAST({parameter} AS INT) AS a, /* outer /* hidden */ CAST({parameter} AS INT) AS b{suffix}"
    );
    let off_sql = format!(
        "SELECT CAST({parameter} AS INT) AS a, /* outer /* hidden */ {parameter} */ CAST({parameter} AS INT) AS b{suffix}"
    );

    // The initial setting must apply to ordinary and explicitly typed paths.
    let statement = client.prepare(&on_sql).await.unwrap();
    assert_eq!(statement.params(), &[Type::INT4, Type::INT4]);
    let row = client
        .query_one(&statement, &[&7_i32, &11_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    let rows = client
        .query_typed(&on_sql, &[(&7_i32, Type::INT4), (&11_i32, Type::INT4)])
        .await
        .unwrap();
    assert_eq!(
        (rows[0].get::<_, i32>(0), rows[0].get::<_, i32>(1)),
        (7, 11)
    );
    assert_eq!(
        client
            .execute_typed(&on_sql, &[(&7_i32, Type::INT4), (&11_i32, Type::INT4)])
            .await
            .unwrap(),
        1
    );
    let overlapping_sql = format!("SELECT /*/*/ {parameter} */ CAST({parameter} AS INT){suffix}");
    let row = client.query_one(&overlapping_sql, &[&7_i32]).await.unwrap();
    assert_eq!(row.get::<_, i32>(0), 7);

    // The second connection uses nested comments while the first uses flat ones.
    let row = other.query_one(&off_sql, &[&7_i32, &11_i32]).await.unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    client
        .batch_execute("SET enable_unpaired_comment = off")
        .await
        .unwrap();
    let row = client
        .query_one(&off_sql, &[&7_i32, &11_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    // Existing statements retain their parsed SQL when the setting changes.
    let row = client
        .query_one(&statement, &[&13_i32, &17_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (13, 17));

    let transaction = client.transaction().await.unwrap();
    transaction
        .batch_execute("SET LOCAL enable_unpaired_comment = on")
        .await
        .unwrap();
    let row = transaction
        .query_one(&on_sql, &[&7_i32, &11_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    transaction.rollback().await.unwrap();
    let row = client
        .query_one(&off_sql, &[&7_i32, &11_i32])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    client
        .batch_execute("SET enable_unpaired_comment = on")
        .await
        .unwrap();
    let row = client.query_one(&on_sql, &[&7_i32, &11_i32]).await.unwrap();
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i32>(1)), (7, 11));
    client.check_connection().await.unwrap();
}

#[tokio::test]
#[ignore = "requires KINGBASE_MYSQL_URL with permission to set enable_unpaired_comment"]
async fn mysql_live_unpaired_comment_settings() {
    verify_unpaired_comment_settings("KINGBASE_MYSQL_URL", CompatibleMode::Mysql, "?").await;
}

#[tokio::test]
#[ignore = "requires KINGBASE_ORACLE_URL with permission to set enable_unpaired_comment"]
async fn oracle_live_unpaired_comment_settings() {
    verify_unpaired_comment_settings("KINGBASE_ORACLE_URL", CompatibleMode::Oracle, ":value").await;
}
