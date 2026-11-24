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
// test_5000_execution_warning()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

/// The warning the database returns for a unit created with compilation
/// errors.
const COMPILATION_WARNING: &str = "creation succeeded with compilation errors";

/// A procedure that is created with compilation errors.
const INVALID_UNIT: &str =
    "create or replace procedure test_5000_proc is begin no_such_thing; end;";

/// The same procedure without compilation errors.
const VALID_UNIT: &str =
    "create or replace procedure test_5000_proc is begin null; end;";

/// Drops the procedure created by the tests.
fn drop_unit(conn: &oracledb::Connection) -> Result<(), oracledb::Error> {
    conn.execute(
        "begin
            execute immediate 'drop procedure test_5000_proc';
        exception
            when others then
                if sqlcode != -4043 then
                    raise;
                end if;
        end;",
        &[],
    )?;
    Ok(())
}

#[rstest]
/// Tests that the same warning returned by two executions in a row is
/// reported by each execution.
fn test_5000(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let first = conn.execute(INVALID_UNIT, &[]);
    let again = conn.execute(INVALID_UNIT, &[]);
    drop_unit(&conn)?;
    assert_eq!(first?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(again?.warning(), Some(COMPILATION_WARNING));
    Ok(())
}

#[rstest]
/// Tests that an execution that returns no warning reports none, even right
/// after one that did, while the connection's last warning is unchanged.
fn test_5001(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let warned = conn.execute(INVALID_UNIT, &[]);
    let valid = conn.execute(VALID_UNIT, &[]);
    assert_eq!(warned?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(valid?.warning(), None);

    let warned = conn.execute(INVALID_UNIT, &[]);
    let plsql = conn.execute("begin null; end;", &[]);
    let cursor = conn.query("select 1 from dual", &[]);
    let last_warning = conn.last_warning();
    drop_unit(&conn)?;
    assert_eq!(warned?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(plsql?.warning(), None);
    assert_eq!(cursor?.warning(), None);
    assert_eq!(last_warning?.as_deref(), Some(COMPILATION_WARNING));
    Ok(())
}

#[rstest]
/// Tests that execute_batch() reports the warning of its own execution.
fn test_5002(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let first =
        conn.execute_batch(INVALID_UNIT, oracledb::BindParameters::default());
    let again =
        conn.execute_batch(INVALID_UNIT, oracledb::BindParameters::default());
    let valid =
        conn.execute_batch(VALID_UNIT, oracledb::BindParameters::default());
    drop_unit(&conn)?;
    assert_eq!(first?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(again?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(valid?.warning(), None);
    Ok(())
}

#[test]
/// Tests that a warning returned on a pooled connection is not reported by
/// the next connection acquired from the pool with the same session.
fn test_5003() -> Result<(), oracledb::Error> {
    let test_config = oracledb::get_test_config();
    let pool = oracledb::create_pool(
        oracledb::PoolConfig::default()
            .set_credentials(&test_config.user, &test_config.password)
            .set_connect_string(&test_config.connect_string)?
            .set_min_connections(0)
            .set_max_connections(1)
            .set_connection_increment(1),
    )?;
    let mut first = pool.acquire()?;
    let session = (first.session_id()?, first.serial_num()?);
    let warned = first.execute(INVALID_UNIT, &[]);
    first.close()?;

    let mut next = pool.acquire()?;
    assert_eq!((next.session_id()?, next.serial_num()?), session);
    let valid = next.execute(VALID_UNIT, &[]);
    let cursor = next.query("select 1 from dual", &[]);
    drop_unit(&next)?;
    next.close()?;
    assert_eq!(warned?.warning(), Some(COMPILATION_WARNING));
    assert_eq!(valid?.warning(), None);
    assert_eq!(cursor?.warning(), None);
    Ok(())
}
