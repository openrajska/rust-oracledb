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
// test_5500_dml_returning_lobs_objects.rs
//
// DML RETURNING of LOBs, objects and collections.
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

#[rstest]
/// Tests DML RETURNING of an object for one row and for several rows, atomic
/// NULL included.
fn test_5503(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _type_guard = common::create_type(
        &conn,
        "test_5503_point",
        "object (x number, label varchar2(10))",
    )?;
    let _guard = common::create_table(
        &conn,
        "test_5503",
        "id number, p test_5503_point",
    )?;
    let point_type = conn.get_type("TEST_5503_POINT")?;

    let mut result = conn.execute_named(
        r#"
        insert into test_5503 values (1, test_5503_point(1.5, 'one'))
        returning p into :p
        "#,
        &[("p", &point_type)],
    )?;
    let mut rows = result.returned_data();
    assert_eq!(rows.len(), 1);
    let point: oracledb::DbObject = rows[0].take(0)?;
    assert_eq!(point.get::<f64>("X")?, 1.5);
    assert_eq!(point.get::<String>("LABEL")?, "one");

    conn.execute("insert into test_5503 values (2, null)", &[])?;
    conn.execute(
        "insert into test_5503 values (3, test_5503_point(null, 'three'))",
        &[],
    )?;
    let mut result = conn.execute_named(
        "update test_5503 set id = id returning id, p into :id, :p",
        &[("id", &oracledb::DB_TYPE_NUMBER), ("p", &point_type)],
    )?;
    let mut returned = Vec::new();
    for mut row in result.returned_data() {
        let id: i32 = row.get(0)?;
        let point = row.take::<Option<oracledb::DbObject>>(1)?.map(|p| {
            (
                p.get::<Option<f64>>("X").unwrap(),
                p.get::<String>("LABEL").unwrap(),
            )
        });
        returned.push((id, point));
    }
    returned.sort_by_key(|(id, _)| *id);
    assert_eq!(
        returned,
        vec![
            (1, Some((Some(1.5), "one".to_string()))),
            (2, None),
            (3, Some((None, "three".to_string()))),
        ]
    );
    Ok(())
}

#[rstest]
/// Tests DML RETURNING of a VARRAY column and of a nested table built from a
/// column for several rows, NULL and empty collections included.
fn test_5504(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _numbers_guard = common::create_type(
        &conn,
        "test_5504_numbers",
        "varray(10) of number",
    )?;
    let _tags_guard =
        common::create_type(&conn, "test_5504_tags", "table of varchar2(20)")?;
    let _guard = common::create_table(
        &conn,
        "test_5504",
        "id number, n test_5504_numbers, label varchar2(10)",
    )?;
    conn.execute(
        r#"
        insert into test_5504 values
            (1, test_5504_numbers(1, 2, 3), 'a')
        "#,
        &[],
    )?;
    conn.execute("insert into test_5504 values (2, null, null)", &[])?;
    conn.execute(
        r#"
        insert into test_5504 values
            (3, test_5504_numbers(), 'c')
        "#,
        &[],
    )?;
    let numbers_type = conn.get_type("TEST_5504_NUMBERS")?;
    let tags_type = conn.get_type("TEST_5504_TAGS")?;
    let mut result = conn.execute_named(
        r#"
        update test_5504 set id = id
        returning
            n,
            id,
            case
                when id = 3 then test_5504_tags()
                when label is not null then test_5504_tags(label, upper(label))
            end
        into :n, :id, :t
        "#,
        &[
            ("n", &numbers_type),
            ("id", &oracledb::DB_TYPE_NUMBER),
            ("t", &tags_type),
        ],
    )?;
    let mut returned = Vec::new();
    for mut row in result.returned_data() {
        let numbers = row
            .take::<Option<oracledb::DbCollection>>(0)?
            .map(|c| Vec::<i32>::try_from(c).unwrap());
        let id: i32 = row.get(1)?;
        let tags = row
            .take::<Option<oracledb::DbCollection>>(2)?
            .map(|c| Vec::<String>::try_from(c).unwrap());
        returned.push((id, numbers, tags));
    }
    returned.sort_by_key(|(id, _, _)| *id);
    assert_eq!(
        returned,
        vec![
            (
                1,
                Some(vec![1, 2, 3]),
                Some(vec!["a".to_string(), "A".to_string()])
            ),
            (2, None, None),
            (3, Some(vec![]), Some(vec![])),
        ]
    );
    Ok(())
}

#[rstest]
/// Tests DML RETURNING of LOBs and objects grouped by execute_batch
/// invocation, an execution that affects no rows included.
fn test_5505(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _type_guard =
        common::create_type(&conn, "test_5505_point", "object (x number)")?;
    let _guard = common::create_table(
        &conn,
        "test_5505",
        "id number, c clob, p test_5505_point",
    )?;
    conn.execute(
        "insert into test_5505 values (1, to_clob('one'), test_5505_point(1))",
        &[],
    )?;
    conn.execute("insert into test_5505 values (2, null, null)", &[])?;
    conn.execute(
        r#"
        insert into test_5505 values (3, to_clob('three'), test_5505_point(3))
        "#,
        &[],
    )?;
    let point_type = conn.get_type("TEST_5505_POINT")?;
    let params = oracledb::BindParameters::Slice(&[
        &[&1, &oracledb::DB_TYPE_CLOB, &point_type],
        &[&2, &oracledb::DB_TYPE_CLOB, &point_type],
        &[&4, &oracledb::DB_TYPE_CLOB, &point_type],
        &[&3, &oracledb::DB_TYPE_CLOB, &point_type],
    ]);
    let mut result = conn.execute_batch(
        r#"
        update test_5505 set id = id where id = :1
        returning c, p into :2, :3
        "#,
        params,
    )?;
    let mut returned = Vec::new();
    for rows in result.returned_data() {
        let mut execution = Vec::new();
        for mut row in rows {
            let c = clob_text(row.take(0)?);
            let x = row
                .take::<Option<oracledb::DbObject>>(1)?
                .map(|p| p.get::<i32>("X").unwrap());
            execution.push((c, x));
        }
        returned.push(execution);
    }
    assert_eq!(
        returned,
        vec![
            vec![(Some("one".to_string()), Some(1))],
            vec![(None, None)],
            vec![],
            vec![(Some("three".to_string()), Some(3))],
        ]
    );
    Ok(())
}
