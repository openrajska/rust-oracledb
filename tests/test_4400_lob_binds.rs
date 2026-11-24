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
// test_4400_lob_binds()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;
use std::io::Write;

/// The columns of the tables the tests insert into.
const COLUMNS: &str = "id number(9), c clob, n nclob, b blob";

/// Returns text of about 1 MiB in UTF-8, well over the 32767 bytes a string
/// bind can carry into PL/SQL, mixing 1-, 2-, 3- and 4-byte characters.
fn large_text() -> String {
    "aç€😀".repeat(100_000)
}

/// Returns about 1 MiB of bytes covering every byte value.
fn large_bytes() -> Vec<u8> {
    (0..1_000_000).map(|i| (i % 256) as u8).collect()
}

/// Creates a temporary LOB of the given type holding the given data.
fn temp_lob(
    conn: &oracledb::Connection,
    db_type: &'static oracledb::DbType,
    data: &[u8],
) -> Result<oracledb::Lob, oracledb::Error> {
    let mut lob = conn.create_lob(db_type)?;
    // character LOB writes need complete UTF-8 in each call
    let chunk_size = if db_type == oracledb::DB_TYPE_BLOB {
        65_536
    } else {
        data.len().max(1)
    };
    for chunk in data.chunks(chunk_size) {
        lob.write_all(chunk)?;
    }
    Ok(lob)
}

/// Creates temporary CLOB, NCLOB and BLOB values holding the given data.
fn temp_lobs(
    conn: &oracledb::Connection,
    text: &str,
    bytes: &[u8],
) -> Result<[oracledb::Lob; 3], oracledb::Error> {
    Ok([
        temp_lob(conn, oracledb::DB_TYPE_CLOB, text.as_bytes())?,
        temp_lob(conn, oracledb::DB_TYPE_NCLOB, text.as_bytes())?,
        temp_lob(conn, oracledb::DB_TYPE_BLOB, bytes)?,
    ])
}

/// Asserts that the row with the given id holds the given text in its CLOB
/// and NCLOB columns and the given bytes in its BLOB column.
fn assert_row(
    conn: &oracledb::Connection,
    table_name: &str,
    id: i32,
    text: &str,
    bytes: &[u8],
) -> Result<(), oracledb::Error> {
    let sql = format!(
        "select c, n, b, dbms_lob.getlength(c), dbms_lob.getlength(n),
                dbms_lob.getlength(b)
         from {table_name} where id = :1"
    );
    let row = conn.query_row(&sql, &[&id])?;
    let num_units = text.encode_utf16().count();
    assert_eq!(row.get::<Option<String>>(0)?.unwrap_or_default(), text);
    assert_eq!(row.get::<Option<String>>(1)?.unwrap_or_default(), text);
    assert_eq!(row.get::<Option<Vec<u8>>>(2)?.unwrap_or_default(), bytes);
    assert_eq!(row.get::<usize>(3)?, num_units);
    assert_eq!(row.get::<usize>(4)?, num_units);
    assert_eq!(row.get::<usize>(5)?, bytes.len());
    Ok(())
}

#[rstest]
/// Tests that temporary CLOB, NCLOB and BLOB values bind IN to SQL.
fn test_4400(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4400", COLUMNS)?;
    let (text, bytes) = (large_text(), large_bytes());
    let [c, n, b] = temp_lobs(&conn, &text, &bytes)?;
    conn.execute(
        "insert into test_4400 values (:1, :2, :3, :4)",
        &[&1, &c, &n, &b],
    )?;
    let [c, n, b] = temp_lobs(&conn, "small", b"small")?;
    conn.execute(
        "insert into test_4400 values (:1, :2, :3, :4)",
        &[&2, &c, &n, &b],
    )?;
    assert_row(&conn, "test_4400", 1, &text, &bytes)?;
    assert_row(&conn, "test_4400", 2, "small", b"small")?;
    Ok(())
}

#[rstest]
/// Tests that temporary CLOB, NCLOB and BLOB values bind IN to PL/SQL: to
/// the parameters of a procedure and to an anonymous block's expressions.
fn test_4401(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4401", COLUMNS)?;
    conn.execute(
        "create or replace procedure test_4401_proc (
            a_id number,
            a_c clob,
            a_n nclob,
            a_b blob
        ) is
        begin
            insert into test_4401 values (a_id, a_c, a_n, a_b);
        end;",
        &[],
    )?;
    let (text, bytes) = (large_text(), large_bytes());
    let [c, n, b] = temp_lobs(&conn, &text, &bytes)?;
    conn.execute(
        "begin test_4401_proc(:1, :2, :3, :4); end;",
        &[&1, &c, &n, &b],
    )?;
    conn.execute(
        "begin insert into test_4401 values (:1, :2, :3, :4); end;",
        &[&2, &c, &n, &b],
    )?;
    let mut result = conn.execute(
        "begin :1 := dbms_lob.getlength(:2) + dbms_lob.getlength(:3); end;",
        &[&0, &c, &b],
    )?;
    let expected = text.encode_utf16().count() + bytes.len();
    assert_eq!(result.out_bind_data().get::<usize>(0)?, expected);
    conn.execute("drop procedure test_4401_proc", &[])?;
    assert_row(&conn, "test_4401", 1, &text, &bytes)?;
    assert_row(&conn, "test_4401", 2, &text, &bytes)?;
    Ok(())
}

#[rstest]
/// Tests that an empty temporary LOB binds as an empty LOB, not as NULL.
fn test_4402(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let [c, n, b] = temp_lobs(&conn, "", b"")?;
    let row = conn.query_row(
        "select dbms_lob.getlength(:1), dbms_lob.getlength(:2),
                dbms_lob.getlength(:3)
         from dual",
        &[&c, &n, &b],
    )?;
    assert_eq!(row.get::<Option<i32>>(0)?, Some(0));
    assert_eq!(row.get::<Option<i32>>(1)?, Some(0));
    assert_eq!(row.get::<Option<i32>>(2)?, Some(0));
    Ok(())
}

#[rstest]
/// Tests that LOBs fetched from a table bind back IN.
fn test_4403(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4403", COLUMNS)?;
    let (text, bytes) = (large_text(), large_bytes());
    let [c, n, b] = temp_lobs(&conn, &text, &bytes)?;
    conn.execute(
        "insert into test_4403 values (:1, :2, :3, :4)",
        &[&1, &c, &n, &b],
    )?;
    let mut row = conn
        .statement("select c, n, b from test_4403 where id = 1")?
        .fetch_lobs()
        .build()?
        .query_row(&[])?;
    let c: oracledb::Lob = row.take(0)?;
    let n: oracledb::Lob = row.take(1)?;
    let b: oracledb::Lob = row.take(2)?;
    conn.execute(
        "insert into test_4403 values (:1, :2, :3, :4)",
        &[&2, &c, &n, &b],
    )?;
    conn.execute(
        "begin insert into test_4403 values (:1, :2, :3, :4); end;",
        &[&3, &c, &n, &b],
    )?;
    assert_row(&conn, "test_4403", 2, &text, &bytes)?;
    assert_row(&conn, "test_4403", 3, &text, &bytes)?;
    Ok(())
}

#[rstest]
/// Tests that LOBs bind IN in a batch, next to NULL LOBs.
fn test_4404(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4404", COLUMNS)?;
    let (text, bytes) = (large_text(), large_bytes());
    let [c1, n1, b1] = temp_lobs(&conn, &text, &bytes)?;
    let [c2, n2, b2] = temp_lobs(&conn, "second", b"second")?;
    let params = oracledb::BindParameters::Slice(&[
        &[&1, &c1, &n1, &b1],
        &[&2, &c2, &n2, &b2],
        &[
            &3,
            &oracledb::DB_TYPE_CLOB,
            &oracledb::DB_TYPE_NCLOB,
            &oracledb::DB_TYPE_BLOB,
        ],
    ]);
    let result = conn.execute_batch(
        "insert into test_4404 values (:1, :2, :3, :4)",
        params,
    )?;
    assert_eq!(result.rows_affected(), 3);
    assert_row(&conn, "test_4404", 1, &text, &bytes)?;
    assert_row(&conn, "test_4404", 2, "second", b"second")?;
    let row = conn.query_row(
        "select count(*) from test_4404
         where id = 3 and c is null and n is null and b is null",
        &[],
    )?;
    assert_eq!(row.get::<i32>(0)?, 1);
    Ok(())
}
