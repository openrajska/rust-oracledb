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
// test_3900_implicit_results()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

/// A block that returns two implicit results with different columns: the
/// numbers 1 to 5 and then the letters a to c.
const TWO_RESULTS: &str = "
    declare
        c1 sys_refcursor;
        c2 sys_refcursor;
    begin
        open c1 for
            select level as id, 'n' || level as name
            from dual connect by level <= 5
            order by level;
        dbms_sql.return_result(c1);
        open c2 for
            select chr(96 + level) as letter
            from dual connect by level <= 3
            order by level;
        dbms_sql.return_result(c2);
    end;";

/// More executions than the 300 cursors a session may keep open by default
/// (OPEN_CURSORS), so any leaked cursor makes the server raise ORA-01000.
const NUM_EXECUTIONS: usize = 200;

/// Returns the values of the first column of every row of the cursor.
fn first_column(
    cursor: oracledb::Cursor,
) -> Result<Vec<String>, oracledb::Error> {
    cursor.map(|row| row?.get::<String>(0)).collect()
}

/// Asserts that the connection is still usable and has not run out of
/// cursors.
fn assert_usable(conn: &oracledb::Connection) -> Result<(), oracledb::Error> {
    let row = conn.query_row("select 3900 from dual", &[])?;
    let value: i32 = row.get(0)?;
    assert_eq!(value, 3900);
    Ok(())
}

#[rstest]
/// Tests that implicit results come back as cursors, in the order the block
/// returned them, each with its own columns and rows.
fn test_3900(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let mut result = conn.execute(TWO_RESULTS, &[])?;
    let mut cursors = result.implicit_results();
    assert_eq!(cursors.len(), 2);

    let letters = cursors.pop().unwrap();
    let numbers = cursors.pop().unwrap();
    let names: Vec<&str> =
        numbers.columns().iter().map(|c| c.name()).collect();
    assert_eq!(names, ["ID", "NAME"]);
    let names: Vec<&str> =
        letters.columns().iter().map(|c| c.name()).collect();
    assert_eq!(names, ["LETTER"]);

    assert_eq!(first_column(numbers)?, ["1", "2", "3", "4", "5"]);
    assert_eq!(first_column(letters)?, ["a", "b", "c"]);

    // the cursors were moved out of the result
    assert!(result.implicit_results().is_empty());
    Ok(())
}

#[rstest]
/// Tests that implicit results are fetched lazily and can be read
/// interleaved, in any order.
fn test_3901(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let mut statement =
        conn.statement(TWO_RESULTS)?.fetch_array_size(2).build()?;
    let mut cursors = statement.execute(&[])?.implicit_results();
    let mut letters = cursors.pop().unwrap();
    let mut numbers = cursors.pop().unwrap();

    let next = |cursor: &mut oracledb::Cursor| -> Option<String> {
        cursor.next().map(|row| row.unwrap().get::<String>(0).unwrap())
    };
    assert_eq!(next(&mut letters).as_deref(), Some("a"));
    assert_eq!(next(&mut numbers).as_deref(), Some("1"));
    assert_eq!(next(&mut numbers).as_deref(), Some("2"));
    assert_eq!(next(&mut numbers).as_deref(), Some("3"));
    assert_eq!(next(&mut letters).as_deref(), Some("b"));
    assert_eq!(next(&mut numbers).as_deref(), Some("4"));
    assert_eq!(next(&mut letters).as_deref(), Some("c"));
    assert_eq!(next(&mut letters), None);
    assert_eq!(next(&mut numbers).as_deref(), Some("5"));
    assert_eq!(next(&mut numbers), None);
    Ok(())
}

#[rstest]
/// Tests that OUT binds and implicit results of one block are both returned.
fn test_3902(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql = "
        declare
            c sys_refcursor;
        begin
            :out_value := :in_value * 2;
            open c for select 'only' from dual;
            dbms_sql.return_result(c);
        end;";
    let mut result =
        conn.execute_named(sql, &[("out_value", &0), ("in_value", &21)])?;
    let out_value: i32 = result.out_bind_data().get(0)?;
    assert_eq!(out_value, 42);
    let mut cursors = result.implicit_results();
    assert_eq!(cursors.len(), 1);
    assert_eq!(first_column(cursors.pop().unwrap())?, ["only"]);
    Ok(())
}

#[rstest]
/// Tests that executions without DBMS_SQL.RETURN_RESULT return no implicit
/// results.
fn test_3903(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let mut result = conn.execute("begin null; end;", &[])?;
    assert!(result.implicit_results().is_empty());
    let mut result = conn.execute("select 1 from dual", &[])?;
    assert!(result.implicit_results().is_empty());
    let mut result =
        conn.execute("alter session set nls_language = 'AMERICAN'", &[])?;
    assert!(result.implicit_results().is_empty());
    Ok(())
}

#[rstest]
/// Tests that an error raised after DBMS_SQL.RETURN_RESULT fails the
/// execution and closes the cursors already returned.
fn test_3904(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql = "
        declare
            c sys_refcursor;
        begin
            open c for select 1 from dual;
            dbms_sql.return_result(c);
            open c for select 2 from dual;
            dbms_sql.return_result(c);
            raise_application_error(-20000, 'test_3904');
        end;";
    for _ in 0..NUM_EXECUTIONS {
        let error = conn.execute(sql, &[]).unwrap_err();
        assert_eq!(error.db_error_code(), Some(20000));
    }
    assert_usable(&conn)
}

#[rstest]
/// Tests that implicit results that are not read, or not even taken from
/// the result, are closed when dropped.
fn test_3905(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    for _ in 0..NUM_EXECUTIONS {
        let mut result = conn.execute(TWO_RESULTS, &[])?;
        let cursors = result.implicit_results();
        assert_eq!(cursors.len(), 2);
    }
    for _ in 0..NUM_EXECUTIONS {
        conn.execute(TWO_RESULTS, &[])?;
    }
    assert_usable(&conn)
}

#[rstest]
/// Tests that Connection::query() and Connection::query_named() close the
/// implicit results of a block instead of leaking them.
fn test_3906(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    for _ in 0..NUM_EXECUTIONS {
        let cursor = conn.query(TWO_RESULTS, &[])?;
        assert!(cursor.columns().is_empty());
    }
    for _ in 0..NUM_EXECUTIONS {
        conn.query_named(TWO_RESULTS, &[])?;
    }
    assert_usable(&conn)
}

#[rstest]
/// Tests that execute_batch() returns the implicit results of every
/// execution, in order, including the first one, which is performed on its
/// own the first time a PL/SQL block is executed.
fn test_3907(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql = "
        declare
            c sys_refcursor;
        begin
            open c for select :1 from dual;
            dbms_sql.return_result(c);
        end;";
    let mut statement = conn.statement(sql)?.build()?;
    for _ in 0..2 {
        let params =
            oracledb::BindParameters::Slice(&[&[&"x"], &[&"y"], &[&"z"]]);
        let mut result = statement.execute_batch(params)?;
        let values = result
            .implicit_results()
            .into_iter()
            .map(first_column)
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(values, [["x"], ["y"], ["z"]]);
    }
    Ok(())
}

#[rstest]
/// Tests that execute_batch() closes the implicit results it returns when
/// they are dropped, also when a later execution of the batch fails.
fn test_3908(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql = "
        declare
            c sys_refcursor;
        begin
            open c for select :1 from dual;
            dbms_sql.return_result(c);
            open c for select :1 from dual;
            dbms_sql.return_result(c);
            if :1 = 'fail' then
                raise_application_error(-20000, 'test_3908');
            end if;
        end;";
    for _ in 0..NUM_EXECUTIONS {
        let params = oracledb::BindParameters::Slice(&[&[&"x"], &[&"y"]]);
        conn.execute_batch(sql, params)?;
    }
    for _ in 0..NUM_EXECUTIONS {
        let params = oracledb::BindParameters::Slice(&[&[&"x"], &[&"fail"]]);
        let error = conn
            .statement(sql)?
            .exclude_from_cache()
            .build()?
            .execute_batch(params)
            .unwrap_err();
        assert_eq!(error.db_error_code(), Some(20000));
    }
    assert_usable(&conn)
}

#[rstest]
/// Tests that an implicit result with more rows than the fetch array size
/// returns every row.
fn test_3909(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql = "
        declare
            c sys_refcursor;
        begin
            open c for select level from dual connect by level <= 1000;
            dbms_sql.return_result(c);
        end;";
    let mut statement = conn.statement(sql)?.fetch_array_size(100).build()?;
    let mut cursors = statement.execute(&[])?.implicit_results();
    let values = cursors
        .pop()
        .unwrap()
        .map(|row| row?.get::<i32>(0))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(values.len(), 1000);
    assert_eq!(values.last(), Some(&1000));
    Ok(())
}
