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
// test_5500_dml_returning_lobs.rs
//
// DML RETURNING of LOBs.
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;
use std::io::Read;

/// Reads the whole content of a returned CLOB.
fn clob_text(value: Option<oracledb::Lob>) -> Option<String> {
    value.map(|mut lob| {
        let mut text = String::new();
        lob.read_to_string(&mut text).unwrap();
        text
    })
}

/// Reads the whole content of a returned BLOB.
fn blob_bytes(value: Option<oracledb::Lob>) -> Option<Vec<u8>> {
    value.map(|mut lob| {
        let mut bytes = Vec::new();
        lob.read_to_end(&mut bytes).unwrap();
        bytes
    })
}

#[rstest]
/// Tests DML RETURNING of a CLOB and a BLOB for one row.
fn test_5500(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard =
        common::create_table(&conn, "test_5500", "id number, c clob, b blob")?;
    let mut result = conn.execute_named(
        r#"
        insert into test_5500 values (1, to_clob('café'), hextoraw('0102'))
        returning c, b into :c, :b
        "#,
        &[("c", &oracledb::DB_TYPE_CLOB), ("b", &oracledb::DB_TYPE_BLOB)],
    )?;
    let mut rows = result.returned_data();
    assert_eq!(rows.len(), 1);
    let row = &mut rows[0];
    assert_eq!(clob_text(row.take(0)?), Some("café".to_string()));
    assert_eq!(blob_bytes(row.take(1)?), Some(vec![1, 2]));
    Ok(())
}

#[rstest]
/// Tests DML RETURNING of LOBs for several rows, NULL included, next to a
/// scalar column.
fn test_5501(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard =
        common::create_table(&conn, "test_5501", "id number, c clob, b blob")?;
    conn.execute(
        "insert into test_5501 values (1, to_clob('one'), hextoraw('01'))",
        &[],
    )?;
    conn.execute("insert into test_5501 values (2, null, null)", &[])?;
    conn.execute(
        "insert into test_5501 values (3, empty_clob(), empty_blob())",
        &[],
    )?;
    let mut result = conn.execute_named(
        r#"
        update test_5501 set id = id
        returning c, id, b into :c, :id, :b
        "#,
        &[
            ("c", &oracledb::DB_TYPE_CLOB),
            ("id", &oracledb::DB_TYPE_NUMBER),
            ("b", &oracledb::DB_TYPE_BLOB),
        ],
    )?;
    let mut returned = Vec::new();
    for mut row in result.returned_data() {
        let c = clob_text(row.take(0)?);
        let id: i32 = row.get(1)?;
        let b = blob_bytes(row.take(2)?);
        returned.push((id, c, b));
    }
    returned.sort_by_key(|(id, _, _)| *id);
    assert_eq!(
        returned,
        vec![
            (1, Some("one".to_string()), Some(vec![1])),
            (2, None, None),
            (3, Some(String::new()), Some(Vec::new())),
        ]
    );
    Ok(())
}

#[rstest]
/// Tests DML RETURNING when no rows are affected and the returned column is
/// a LOB.
fn test_5502(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard =
        common::create_table(&conn, "test_5502", "id number, c clob")?;
    let mut result = conn.execute_named(
        "delete from test_5502 returning c into :c",
        &[("c", &oracledb::DB_TYPE_CLOB)],
    )?;
    assert!(result.returned_data().is_empty());
    Ok(())
}
