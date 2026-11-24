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
// test_5300_evolved_object_types.rs
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

/// Fetches the object returned by a single-row query.
fn fetch(
    conn: &oracledb::Connection,
    sql: &str,
) -> Result<oracledb::DbObject, oracledb::Error> {
    conn.query_row(sql, &[])?.take(0)
}

/// Fetches the collection returned by a single-row query.
fn fetch_collection(
    conn: &oracledb::Connection,
    sql: &str,
) -> Result<oracledb::DbCollection, oracledb::Error> {
    conn.query_row(sql, &[])?.take(0)
}

/// Asserts that the result is an error reporting that the named type changed
/// after the connection described it.
fn assert_type_changed<T>(
    result: Result<T, oracledb::Error>,
    type_name: &str,
) {
    let Err(err) = result else {
        panic!("{type_name} was read with a description older than the type");
    };
    match err.kind() {
        oracledb::ErrorKind::DbObjectTypeChanged(name) => {
            assert!(name.ends_with(&format!(".{type_name}")), "{name}");
        }
        _ => panic!("unexpected error: {err}"),
    }
}

#[rstest]
/// Reads values of types evolved by ALTER TYPE in a connection that describes
/// the types after the change.
fn test_5300(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let final_type_name = "TEST_5300_FINAL";
    let _final_type_guard =
        common::create_type(&conn, final_type_name, "object (a number)")?;
    let not_final_type_name = "TEST_5300_NOT_FINAL";
    let _not_final_type_guard = common::create_type(
        &conn,
        not_final_type_name,
        "object (a number) not final",
    )?;
    let list_type_name = "TEST_5300_LIST";
    let _list_type_guard = common::create_type(
        &conn,
        list_type_name,
        &format!("table of {final_type_name}"),
    )?;
    let table_name = "TEST_5300_TAB";
    let _table_guard = common::create_table(
        &conn,
        table_name,
        &format!("id number, value {final_type_name}"),
    )?;
    conn.execute(
        &format!("insert into {table_name} values (1, {final_type_name}(1))"),
        &[],
    )?;
    conn.commit()?;
    for type_name in [final_type_name, not_final_type_name] {
        conn.execute(
            &format!(
                "alter type {type_name} add attribute (b varchar2(10)) \
                 cascade not including table data"
            ),
            &[],
        )?;
    }

    let reader = common::conn();
    for type_name in [final_type_name, not_final_type_name] {
        let sql = format!("select {type_name}(2, 'x') from dual");
        let value = fetch(&reader, &sql)?;
        assert_eq!(value.get::<i32>("A")?, 2);
        assert_eq!(value.get::<String>("B")?, "x");
    }
    let list = fetch_collection(
        &reader,
        &format!(
            "select {list_type_name}({final_type_name}(3, 'y'), \
             {final_type_name}(4, null)) from dual"
        ),
    )?;
    let elements: Vec<oracledb::DbObject> = list.try_into()?;
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].get::<i32>("A")?, 3);
    assert_eq!(elements[0].get::<String>("B")?, "y");
    assert_eq!(elements[1].get::<i32>("A")?, 4);
    assert_eq!(elements[1].get::<Option<String>>("B")?, None);
    let stored = fetch(
        &reader,
        &format!("select value from {table_name} where id = 1"),
    )?;
    assert_eq!(stored.get::<i32>("A")?, 1);
    assert_eq!(stored.get::<Option<String>>("B")?, None);
    Ok(())
}

#[rstest]
/// Fails loudly to read a value of a type that gained an attribute after the
/// connection described it, and keeps the connection usable.
fn test_5301(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let type_name = "TEST_5301_OBJ";
    let _type_guard =
        common::create_type(&conn, type_name, "object (a number)")?;
    let reader = common::conn();
    let value = fetch(&reader, &format!("select {type_name}(1) from dual"))?;
    assert_eq!(value.get::<i32>("A")?, 1);

    conn.execute(
        &format!(
            "alter type {type_name} add attribute (b varchar2(10)) cascade"
        ),
        &[],
    )?;
    let sql = format!("select {type_name}(2, 'x') from dual");
    assert_type_changed(fetch(&reader, &sql), type_name);
    let row = reader.query_row("select 1 from dual", &[])?;
    assert_eq!(row.get::<i32>(0)?, 1);
    let value = fetch(&common::conn(), &sql)?;
    assert_eq!(value.get::<i32>("A")?, 2);
    assert_eq!(value.get::<String>("B")?, "x");
    Ok(())
}

#[rstest]
/// Fails loudly to read a value of a type that lost an attribute after the
/// connection described it.
fn test_5302(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let type_name = "TEST_5302_OBJ";
    let _type_guard = common::create_type(
        &conn,
        type_name,
        "object (a number, b varchar2(10))",
    )?;
    let reader = common::conn();
    let value =
        fetch(&reader, &format!("select {type_name}(1, 'x') from dual"))?;
    assert_eq!(value.get::<String>("B")?, "x");

    conn.execute(
        &format!("alter type {type_name} drop attribute a cascade"),
        &[],
    )?;
    let sql = format!("select {type_name}('y') from dual");
    assert_type_changed(fetch(&reader, &sql), type_name);
    let value = fetch(&common::conn(), &sql)?;
    assert_eq!(value.get::<String>("B")?, "y");
    Ok(())
}

#[rstest]
/// Fails loudly to read an object attribute or a collection element whose
/// type changed after the connection described it, even when the enclosing
/// type is described after the change.
fn test_5303(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let inner_type_name = "TEST_5303_INNER";
    let _inner_type_guard = common::create_type(
        &conn,
        inner_type_name,
        "object (a number, c varchar2(10))",
    )?;
    let outer_type_name = "TEST_5303_OUTER";
    let _outer_type_guard = common::create_type(
        &conn,
        outer_type_name,
        &format!("object (x number, inner {inner_type_name}, z number)"),
    )?;
    let list_type_name = "TEST_5303_LIST";
    let _list_type_guard = common::create_type(
        &conn,
        list_type_name,
        &format!("table of {inner_type_name}"),
    )?;
    let reader = common::conn();
    let value = fetch(
        &reader,
        &format!("select {inner_type_name}(1, 'c') from dual"),
    )?;
    assert_eq!(value.get::<String>("C")?, "c");

    conn.execute(
        &format!(
            "alter type {inner_type_name} add attribute (b varchar2(10)) \
             cascade"
        ),
        &[],
    )?;
    let outer_sql = format!(
        "select {outer_type_name}(2, {inner_type_name}(3, 'c', 'b'), 4) \
         from dual"
    );
    assert_type_changed(fetch(&reader, &outer_sql), inner_type_name);
    let list_sql = format!(
        "select {list_type_name}({inner_type_name}(5, 'c', 'b')) from dual"
    );
    assert_type_changed(fetch_collection(&reader, &list_sql), inner_type_name);

    let fresh = common::conn();
    let mut outer = fetch(&fresh, &outer_sql)?;
    let inner: oracledb::DbObject = outer.take("INNER")?;
    assert_eq!(inner.get::<String>("B")?, "b");
    assert_eq!(outer.get::<i32>("Z")?, 4);
    let list: Vec<oracledb::DbObject> =
        fetch_collection(&fresh, &list_sql)?.try_into()?;
    assert_eq!(list[0].get::<String>("B")?, "b");
    Ok(())
}
