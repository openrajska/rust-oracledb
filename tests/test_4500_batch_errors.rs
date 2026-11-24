//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// test_4500_batch_errors()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

const TABLE_DEFINITION: &str = "
    id number(9) primary key,
    value varchar2(30) not null,
    quantity number(9) check (quantity >= 0)";

const INSERT_SQL: &str = "insert into test_4500 (id, value, quantity)
                          values (:1, :2, :3)";

/// Returns the codes and row offsets of the given batch errors.
fn codes_and_offsets(errors: &[oracledb::DbError]) -> Vec<(usize, usize)> {
    errors.iter().map(|e| (e.offset(), e.code())).collect()
}

/// Returns the number of rows in the table test_4500.
fn count_rows(conn: &oracledb::Connection) -> Result<i64, oracledb::Error> {
    conn.query_row("select count(*) from test_4500", &[])?.get(0)
}

/// Inserts the given number of rows into test_4500 with batch errors
/// enabled. Every row whose index is a multiple of three repeats the id of
/// the row before it, so it fails with ORA-00001. Returns the result.
fn insert_with_duplicates(
    conn: &oracledb::Connection,
    num_rows: usize,
) -> Result<oracledb::ExecBatchResult, oracledb::Error> {
    let ids: Vec<i64> = (0..num_rows as i64)
        .map(|i| if i > 0 && i % 3 == 0 { i - 1 } else { i })
        .collect();
    let rows: Vec<[&dyn oracledb::ToDbValue; 3]> =
        ids.iter().map(|id| [id as _, &"value" as _, &1 as _]).collect();
    let slices: Vec<&[&dyn oracledb::ToDbValue]> =
        rows.iter().map(|r| r.as_slice()).collect();
    conn.statement(INSERT_SQL)?
        .batch_errors()
        .build()?
        .execute_batch(oracledb::BindParameters::Slice(&slices))
}

#[rstest]
/// test a batch reports every failing row with its offset, code and message
/// and applies the other rows
fn test_4500(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let null_value: Option<&str> = None;
    let rows: [&[&dyn oracledb::ToDbValue]; 8] = [
        &[&0, &"zero", &1],
        &[&1, &"one", &1],
        &[&2, &"two", &1],
        &[&1, &"duplicate id", &1],
        &[&4, &"four", &1],
        &[&5, &null_value, &1],
        &[&6, &"six", &1],
        &[&7, &"negative quantity", &-1],
    ];
    let mut statement = conn.statement(INSERT_SQL)?.batch_errors().build()?;
    let result = statement.execute_batch(rows.as_slice().into())?;
    assert_eq!(
        codes_and_offsets(result.batch_errors()),
        vec![(3, 1), (5, 1400), (7, 2290)]
    );
    let messages: Vec<&str> =
        result.batch_errors().iter().map(|e| e.message()).collect();
    assert!(messages[0].starts_with("ORA-00001: "), "{}", messages[0]);
    assert!(messages[1].starts_with("ORA-01400: "), "{}", messages[1]);
    assert!(messages[2].starts_with("ORA-02290: "), "{}", messages[2]);
    assert_eq!(result.rows_affected(), 5);
    assert_eq!(count_rows(&conn)?, 5);
    Ok(())
}

#[rstest]
/// test a batch with batch errors enabled and no failing row reports none
fn test_4501(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let rows: [&[&dyn oracledb::ToDbValue]; 3] =
        [&[&0, &"zero", &1], &[&1, &"one", &1], &[&2, &"two", &1]];
    let mut statement = conn.statement(INSERT_SQL)?.batch_errors().build()?;
    let result = statement.execute_batch(rows.as_slice().into())?;
    assert!(result.batch_errors().is_empty());
    assert_eq!(result.rows_affected(), 3);
    Ok(())
}

#[rstest]
/// test a statement executed again from the statement cache still reports
/// every failing row
fn test_4502(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let first = insert_with_duplicates(&conn, 7)?;
    assert_eq!(codes_and_offsets(first.batch_errors()), vec![(3, 1), (6, 1)]);
    conn.rollback()?;
    let second = insert_with_duplicates(&conn, 7)?;
    assert_eq!(codes_and_offsets(second.batch_errors()), vec![(3, 1), (6, 1)]);
    assert_eq!(second.rows_affected(), 5);
    Ok(())
}

#[rstest]
/// test a batch with many failing rows reports every one of them
fn test_4503(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let result = insert_with_duplicates(&conn, 3000)?;
    let expected: Vec<(usize, usize)> =
        (3..3000).step_by(3).map(|offset| (offset, 1)).collect();
    assert_eq!(codes_and_offsets(result.batch_errors()), expected);
    assert!(
        result
            .batch_errors()
            .iter()
            .all(|e| e.message().starts_with("ORA-00001: "))
    );
    assert_eq!(result.rows_affected(), 2001);
    assert_eq!(count_rows(&conn)?, 2001);
    Ok(())
}

#[rstest]
/// test execute() of a statement with batch errors enabled still returns the
/// error of its row
fn test_4504(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let mut statement = conn.statement(INSERT_SQL)?.batch_errors().build()?;
    statement.execute(&[&1, &"one", &1])?;
    let error = statement
        .execute(&[&1, &"duplicate id", &1])
        .expect_err("a duplicate id must fail execute()");
    assert_eq!(error.db_error_code(), Some(1));
    assert_eq!(count_rows(&conn)?, 1);
    Ok(())
}

#[rstest]
/// test a PL/SQL batch with batch errors enabled stops at its first failing
/// row, as without them
fn test_4505(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let rows: [&[&dyn oracledb::ToDbValue]; 3] =
        [&[&0, &"zero", &1], &[&0, &"duplicate id", &1], &[&2, &"two", &1]];
    let error = conn
        .statement(
            "begin
                insert into test_4500 (id, value, quantity)
                values (:1, :2, :3);
            end;",
        )?
        .batch_errors()
        .build()?
        .execute_batch(rows.as_slice().into())
        .expect_err("a duplicate id must fail a PL/SQL batch");
    assert_eq!(error.db_error_code(), Some(1));
    Ok(())
}

#[rstest]
/// test DML RETURNING with batch errors returns the data of the rows that
/// succeeded and none for those that failed
fn test_4506(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let rows: [&[&dyn oracledb::ToDbValue]; 3] =
        [&[&0, &"zero", &0], &[&0, &"duplicate id", &0], &[&2, &"two", &0]];
    let mut result = conn
        .statement(
            "insert into test_4500 (id, value, quantity)
             values (:1, :2, 1)
             returning id * 10 into :3",
        )?
        .batch_errors()
        .build()?
        .execute_batch(rows.as_slice().into())?;
    assert_eq!(codes_and_offsets(result.batch_errors()), vec![(1, 1)]);
    let returned: Vec<Vec<i64>> = result
        .returned_data()
        .iter()
        .map(|rows| rows.iter().map(|r| r.get(0).unwrap()).collect())
        .collect();
    assert_eq!(returned, vec![vec![0], vec![], vec![20]]);
    Ok(())
}

#[rstest]
/// test a batch with more failing rows than the server can report returns an
/// error and leaves the connection usable
fn test_4507(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4500", TABLE_DEFINITION)?;
    let null_value: Option<&str> = None;
    let ids: Vec<i64> = (0..70000).collect();
    let rows: Vec<[&dyn oracledb::ToDbValue; 3]> =
        ids.iter().map(|id| [id as _, &null_value as _, &1 as _]).collect();
    let slices: Vec<&[&dyn oracledb::ToDbValue]> =
        rows.iter().map(|r| r.as_slice()).collect();
    let error = conn
        .statement(INSERT_SQL)?
        .batch_errors()
        .build()?
        .execute_batch(oracledb::BindParameters::Slice(&slices))
        .expect_err("70000 failing rows must fail execute_batch()");
    assert_eq!(*error.kind(), oracledb::ErrorKind::TooManyBatchErrors(70000));
    conn.rollback()?;
    assert_eq!(count_rows(&conn)?, 0);
    Ok(())
}
