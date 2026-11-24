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
// test_5400_cursor_lobs()
//-----------------------------------------------------------------------------

mod common;

use common::{admin_conn, conn};
use rstest::*;
use std::io::Read;

/// Text mixing 1-, 2-, 3- and 4-byte UTF-8 characters: 18 UCS-2 units, as
/// each 4-byte character is a surrogate pair.
const TEXT: &str = "Ação 日本 😀 b😀😀 €";

/// The number of times the text is repeated in the large value: 36000 UCS-2
/// units, more than the largest string a define can hold inline.
const LARGE_REPEATS: usize = 2000;

/// The query whose rows the tests fetch through a REF CURSOR or a nested
/// cursor.
const QUERY: &str = "select id, c, n, b from test_5400 order by id";

/// The values of one row: the id, then the CLOB, NCLOB and BLOB values.
type Values = (i32, Option<String>, Option<String>, Option<Vec<u8>>);

/// Creates the table used by the tests with one row per kind of value: a
/// multibyte text, empty LOBs, NULL and a large multibyte text.
fn create_table(
    conn: &oracledb::Connection,
) -> Result<common::TableGuard<'_>, oracledb::Error> {
    let guard = common::create_table(
        conn,
        "test_5400",
        "id number(9), c clob, n nclob, b blob",
    )?;
    conn.execute(
        "insert into test_5400 values (1, :1, :2, :3)",
        &[&TEXT, &TEXT, &TEXT.as_bytes().to_vec()],
    )?;
    conn.execute(
        r#"
        insert into test_5400
        values (2, empty_clob(), empty_clob(), empty_blob())
        "#,
        &[],
    )?;
    conn.execute("insert into test_5400 values (3, null, null, null)", &[])?;
    conn.execute_named(
        r#"
        declare
            c clob;
            n nclob;
            b blob;
        begin
            dbms_lob.createtemporary(b, true);
            for i in 1 .. :repeats loop
                c := c || :text;
                n := n || to_nchar(:text);
                dbms_lob.append(b, to_blob(utl_raw.cast_to_raw(:text)));
            end loop;
            insert into test_5400 values (4, c, n, b);
        end;
        "#,
        &[("repeats", &(LARGE_REPEATS as i32)), ("text", &TEXT)],
    )?;
    conn.commit()?;
    Ok(guard)
}

/// Returns the values the rows of the table hold, as fetched with LOB
/// locators.
fn expected_values() -> Vec<Values> {
    let large = TEXT.repeat(LARGE_REPEATS);
    vec![
        (
            1,
            Some(TEXT.to_string()),
            Some(TEXT.to_string()),
            Some(TEXT.as_bytes().to_vec()),
        ),
        (2, Some(String::new()), Some(String::new()), Some(Vec::new())),
        (3, None, None, None),
        (
            4,
            Some(large.clone()),
            Some(large.clone()),
            Some(large.into_bytes()),
        ),
    ]
}

/// Reads a LOB to its end, with a buffer large enough for its whole value so
/// that no read ends inside a surrogate pair.
fn read_lob(lob: &mut oracledb::Lob) -> Result<Vec<u8>, oracledb::Error> {
    let mut buf = vec![0; 3 * lob.get_size()? + 3];
    let mut data = Vec::new();
    loop {
        let num_bytes = lob.read(&mut buf)?;
        if num_bytes == 0 {
            break;
        }
        data.extend_from_slice(&buf[..num_bytes]);
    }
    Ok(data)
}

/// Reads a CLOB or NCLOB column fetched as a LOB locator.
fn text_of(
    row: &mut oracledb::Row,
    index: usize,
) -> Result<Option<String>, oracledb::Error> {
    let lob: Option<oracledb::Lob> = row.take(index)?;
    lob.map(|mut lob| Ok(String::from_utf8(read_lob(&mut lob)?).unwrap()))
        .transpose()
}

/// Reads a BLOB column fetched as a LOB locator.
fn bytes_of(
    row: &mut oracledb::Row,
    index: usize,
) -> Result<Option<Vec<u8>>, oracledb::Error> {
    let lob: Option<oracledb::Lob> = row.take(index)?;
    lob.map(|mut lob| read_lob(&mut lob)).transpose()
}

/// Returns the values of the rows of the cursor, reading the LOB locators
/// when `fetch_lobs` is set and the values directly otherwise.
fn values_of(
    cursor: oracledb::Cursor,
    fetch_lobs: bool,
) -> Result<Vec<Values>, oracledb::Error> {
    let mut values = Vec::new();
    for row in cursor {
        let mut row = row?;
        let id = row.get(0)?;
        if fetch_lobs {
            values.push((
                id,
                text_of(&mut row, 1)?,
                text_of(&mut row, 2)?,
                bytes_of(&mut row, 3)?,
            ));
        } else {
            values.push((id, row.get(1)?, row.get(2)?, row.get(3)?));
        }
    }
    Ok(values)
}

/// Builds a statement for the SQL, fetching LOBs as locators when
/// `fetch_lobs` is set. The small fetch array size makes the rows of the
/// cursors it returns arrive in several fetches.
fn statement(
    conn: &oracledb::Connection,
    sql: &str,
    fetch_lobs: bool,
) -> Result<oracledb::Statement, oracledb::Error> {
    let builder = conn.statement(sql)?.prefetch_rows(3).fetch_array_size(3);
    if fetch_lobs { builder.fetch_lobs().build() } else { builder.build() }
}

/// Returns the values of the rows of the query, run directly.
fn query_values(
    conn: &oracledb::Connection,
    fetch_lobs: bool,
) -> Result<Vec<Values>, oracledb::Error> {
    values_of(statement(conn, QUERY, fetch_lobs)?.query(&[])?, fetch_lobs)
}

/// Returns the values of the rows of the query, fetched through a REF CURSOR
/// opened by a PL/SQL block.
fn ref_cursor_values(
    conn: &oracledb::Connection,
    fetch_lobs: bool,
) -> Result<Vec<Values>, oracledb::Error> {
    let sql = format!("begin open :cursor for {QUERY}; end;");
    let mut result = statement(conn, &sql, fetch_lobs)?
        .execute_named(&[("cursor", &oracledb::DB_TYPE_CURSOR)])?;
    let cursor: oracledb::Cursor = result.out_bind_data().take("cursor")?;
    values_of(cursor, fetch_lobs)
}

/// Returns the values of the rows of the query, fetched through a nested
/// cursor.
fn nested_cursor_values(
    conn: &oracledb::Connection,
    fetch_lobs: bool,
) -> Result<Vec<Values>, oracledb::Error> {
    let sql = format!("select cursor({QUERY}) from dual");
    let mut row = statement(conn, &sql, fetch_lobs)?.query_row(&[])?;
    let cursor: oracledb::Cursor = row.take(0)?;
    values_of(cursor, fetch_lobs)
}

#[rstest]
/// Tests that CLOB, NCLOB and BLOB columns of a REF CURSOR are fetched as LOB
/// locators, like those of a query.
fn test_5400(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn)?;
    assert_eq!(query_values(&conn, true)?, expected_values());
    assert_eq!(ref_cursor_values(&conn, true)?, expected_values());
    Ok(())
}

#[rstest]
/// Tests that CLOB, NCLOB and BLOB columns of a REF CURSOR are fetched as
/// strings and bytes, like those of a query, when LOBs are not fetched as
/// locators.
fn test_5401(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn)?;
    let expected = query_values(&conn, false)?;
    assert_eq!(expected[0], expected_values()[0]);
    assert_eq!(expected[3], expected_values()[3]);
    assert_eq!(ref_cursor_values(&conn, false)?, expected);
    Ok(())
}

#[rstest]
/// Tests that CLOB, NCLOB and BLOB columns of a nested cursor are fetched as
/// LOB locators, like those of a query.
fn test_5402(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn)?;
    assert_eq!(nested_cursor_values(&conn, true)?, expected_values());
    Ok(())
}

#[rstest]
/// Tests that CLOB, NCLOB and BLOB columns of a nested cursor are fetched as
/// strings and bytes, like those of a query, when LOBs are not fetched as
/// locators.
fn test_5403(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn)?;
    let expected = query_values(&conn, false)?;
    assert_eq!(nested_cursor_values(&conn, false)?, expected);
    Ok(())
}

/// Returns the session id of the connection.
fn sid(conn: &oracledb::Connection) -> Result<String, oracledb::Error> {
    conn.query_row("select sys_context('USERENV', 'SID') from dual", &[])?
        .get(0)
}

/// Returns the number of round trips the session has made so far, as seen
/// from the admin connection (which costs the session no round trip).
fn round_trips(
    admin_conn: &oracledb::Connection,
    sid: &str,
) -> Result<i64, oracledb::Error> {
    admin_conn
        .query_row(
            r#"
            select s.value
            from v$sesstat s
                join v$statname n on n.statistic# = s.statistic#
            where s.sid = :1
              and n.name = 'SQL*Net roundtrips to/from client'
            "#,
            &[&sid],
        )?
        .get(0)
}

/// Opens a REF CURSOR for the query, with the given fetch array size, and
/// returns the number of rows it has and the number of round trips that
/// fetching them took.
fn fetch_round_trips(
    conn: &oracledb::Connection,
    admin_conn: &oracledb::Connection,
    query: &str,
    fetch_array_size: u32,
) -> Result<(usize, i64), oracledb::Error> {
    let sid = sid(conn)?;
    let mut result = conn
        .statement(&format!("begin open :cursor for {query}; end;"))?
        .fetch_array_size(fetch_array_size)
        .fetch_lobs()
        .build()?
        .execute_named(&[("cursor", &oracledb::DB_TYPE_CURSOR)])?;
    let cursor: oracledb::Cursor = result.out_bind_data().take("cursor")?;
    let before = round_trips(admin_conn, &sid)?;
    let num_rows = cursor.collect::<Result<Vec<_>, _>>()?.len();
    Ok((num_rows, round_trips(admin_conn, &sid)? - before))
}

#[rstest]
/// Tests that the define sent with the first fetch of a REF CURSOR with LOB
/// columns costs no round trip: its rows take as many round trips as those
/// of a REF CURSOR without LOB columns.
fn test_5404(
    conn: oracledb::Connection,
    admin_conn: oracledb::Connection,
) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn)?;
    for filter in ["", "where id < 0"] {
        let with_lobs = format!("select id, c, n, b from test_5400 {filter}");
        let without_lobs = format!("select id from test_5400 {filter}");
        for fetch_array_size in [1, 3, 100] {
            assert_eq!(
                fetch_round_trips(
                    &conn,
                    &admin_conn,
                    &with_lobs,
                    fetch_array_size
                )?,
                fetch_round_trips(
                    &conn,
                    &admin_conn,
                    &without_lobs,
                    fetch_array_size
                )?,
                "{with_lobs} with fetch array size {fetch_array_size}"
            );
        }
    }
    Ok(())
}

#[rstest]
/// Tests that JSON columns of a REF CURSOR, which are part of the define its
/// first fetch sends, are fetched like those of a query.
fn test_5405(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_json_supported(&conn) {
        return Ok(());
    }
    let query = "select json_object('key' value 'Ação 日本 😀' returning json) \
                 from dual";
    let expected: oracledb::JsonValue = conn.query_row(query, &[])?.get(0)?;
    let mut result = conn.execute_named(
        &format!("begin open :cursor for {query}; end;"),
        &[("cursor", &oracledb::DB_TYPE_CURSOR)],
    )?;
    let cursor: oracledb::Cursor = result.out_bind_data().take("cursor")?;
    let values = cursor
        .map(|row| row?.get(0))
        .collect::<Result<Vec<oracledb::JsonValue>, _>>()?;
    assert_eq!(values, vec![expected]);
    Ok(())
}
