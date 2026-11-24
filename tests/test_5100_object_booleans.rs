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
// test_5100_object_booleans.rs
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

/// Checks the three BOOLEAN attributes of an object built from
/// (true, false, null).
fn assert_true_false_null(
    value: &oracledb::DbObject,
) -> Result<(), oracledb::Error> {
    assert_eq!(value.get::<Option<bool>>("YES")?, Some(true));
    assert_eq!(value.get::<Option<bool>>("NO")?, Some(false));
    assert_eq!(value.get::<Option<bool>>("UNKNOWN")?, None);
    Ok(())
}

#[rstest]
/// Reads the BOOLEAN attributes of an object built in SQL.
fn test_5100(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_boolean_supported(&conn) {
        return Ok(());
    }
    let type_name = "TEST_5100_OBJ";
    let _type_guard = common::create_type(
        &conn,
        type_name,
        "object (yes boolean, no boolean, unknown boolean)",
    )?;
    let sql = format!("select {type_name}(true, false, null) from dual");
    let value: oracledb::DbObject = conn.query_row(&sql, &[])?.take(0)?;
    assert_true_false_null(&value)
}

#[rstest]
/// Reads the BOOLEAN attributes of an object stored in a table.
fn test_5101(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_boolean_supported(&conn) {
        return Ok(());
    }
    let type_name = "TEST_5101_OBJ";
    let _type_guard = common::create_type(
        &conn,
        type_name,
        "object (yes boolean, no boolean, unknown boolean)",
    )?;
    let table_name = "TEST_5101_TAB";
    let _table_guard = common::create_table(
        &conn,
        table_name,
        &format!("id number, value {type_name}"),
    )?;
    conn.execute(
        &format!(
            "insert into {table_name} values (1, {type_name}(true, false, null))"
        ),
        &[],
    )?;
    let sql = format!("select value from {table_name} where id = 1");
    let value: oracledb::DbObject = conn.query_row(&sql, &[])?.take(0)?;
    assert_true_false_null(&value)
}

#[rstest]
/// Reads the BOOLEAN attributes of an object built in PL/SQL.
fn test_5102(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_boolean_supported(&conn) {
        return Ok(());
    }
    let type_name = "TEST_5102_OBJ";
    let _type_guard = common::create_type(
        &conn,
        type_name,
        "object (yes boolean, no boolean, unknown boolean)",
    )?;
    let sql = format!(
        r#"
        with function build return {type_name} is
            value {type_name} := {type_name}(null, null, null);
        begin
            value.yes := true;
            value.no := false;
            return value;
        end;
        select build() from dual
        "#,
    );
    let value: oracledb::DbObject = conn.query_row(&sql, &[])?.take(0)?;
    assert_true_false_null(&value)
}

#[rstest]
/// Reads the BOOLEAN elements of collections built in SQL and in PL/SQL.
fn test_5103(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_boolean_supported(&conn) {
        return Ok(());
    }
    let type_name = "TEST_5103_LIST";
    let _type_guard =
        common::create_type(&conn, type_name, "table of boolean")?;
    let sql = format!(
        r#"
        with function build return {type_name} is
            value {type_name} := {type_name}();
        begin
            value.extend(3);
            value(1) := true;
            value(2) := false;
            return value;
        end;
        select {type_name}(true, false, null), build() from dual
        "#,
    );
    let mut row = conn.query_row(&sql, &[])?;
    for index in 0..2 {
        let list: oracledb::DbCollection = row.take(index)?;
        let values: Vec<Option<bool>> = list.try_into()?;
        assert_eq!(values, vec![Some(true), Some(false), None]);
    }
    Ok(())
}
