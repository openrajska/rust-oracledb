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
// test_5600_transaction_in_progress()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

/// Returns whether the database has a transaction open for the session,
/// asking it directly.
fn server_transaction(conn: &oracledb::Connection) -> bool {
    let row = conn
        .query_row(
            "select dbms_transaction.local_transaction_id from dual",
            &[],
        )
        .unwrap();
    let transaction_id: Option<String> = row.get(0).unwrap();
    transaction_id.is_some()
}

#[rstest]
/// Tests that the transaction state follows what the database reports.
fn test_5600(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(
        &conn,
        "test_5600",
        "id number primary key, value varchar2(30)",
    )?;
    conn.commit()?;
    assert!(!conn.transaction_in_progress()?);

    let cases = [
        ("insert into test_5600 values (1, 'one')", true),
        ("commit", false),
        ("update test_5600 set value = 'uno'", true),
        ("rollback", false),
        ("select id from test_5600", false),
        ("select id from test_5600 for update", true),
        ("rollback", false),
        ("begin insert into test_5600 values (2, 'two'); end;", true),
        ("rollback", false),
        (
            "begin insert into test_5600 values (3, 'three'); commit; end;",
            false,
        ),
        ("set transaction read only", true),
        ("commit", false),
        ("lock table test_5600 in exclusive mode", true),
        ("rollback", false),
    ];
    for (sql, expected) in cases {
        if sql.starts_with("select") {
            conn.query(sql, &[])?.next().unwrap()?;
        } else {
            conn.execute(sql, &[])?;
        }
        assert_eq!(conn.transaction_in_progress()?, expected, "after {sql}");
        assert_eq!(server_transaction(&conn), expected, "after {sql}");
    }

    conn.execute("insert into test_5600 values (4, 'four')", &[])?;
    conn.commit()?;
    assert!(!conn.transaction_in_progress()?);
    conn.execute("delete from test_5600 where id = 4", &[])?;
    assert!(conn.transaction_in_progress()?);
    conn.rollback()?;
    assert!(!conn.transaction_in_progress()?);
    Ok(())
}

#[rstest]
/// Tests that the transaction state follows what the database reports when
/// a call fails.
fn test_5601(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(
        &conn,
        "test_5601",
        "id number constraint test_5601_ck check (id > 0) \
         deferrable initially deferred",
    )?;

    // a failing commit (ORA-02091) rolls the transaction back
    conn.execute("insert into test_5601 values (0)", &[])?;
    assert!(conn.transaction_in_progress()?);
    let err = conn.commit().expect_err("the deferred check must fail");
    assert_eq!(err.db_error_code(), Some(2091));
    assert!(!conn.transaction_in_progress()?);
    assert!(!server_transaction(&conn));

    // a failing statement inside a transaction leaves it open
    conn.execute("insert into test_5601 values (1)", &[])?;
    let err = conn
        .execute("insert into test_5601 values ('x')", &[])
        .expect_err("the insert must fail");
    assert_eq!(err.db_error_code(), Some(1722));
    assert!(conn.transaction_in_progress()?);
    assert!(server_transaction(&conn));
    conn.rollback()?;

    // a failing statement as the first one leaves no transaction open
    let err = conn
        .execute("insert into test_5601 values ('x')", &[])
        .expect_err("the insert must fail");
    assert_eq!(err.db_error_code(), Some(1722));
    assert!(!conn.transaction_in_progress()?);
    assert!(!server_transaction(&conn));
    Ok(())
}
