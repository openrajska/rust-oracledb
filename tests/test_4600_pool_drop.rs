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
// test_4600_pool_drop()
//-----------------------------------------------------------------------------

use std::time::{Duration, Instant};

mod common;

/// Returns the configuration of a pool that grows one connection at a time up
/// to the given maximum.
fn pool_config(
    max_connections: usize,
) -> Result<oracledb::PoolConfig, oracledb::Error> {
    let test_config = oracledb::get_test_config();
    Ok(oracledb::PoolConfig::default()
        .set_credentials(&test_config.user, &test_config.password)
        .set_connect_string(&test_config.connect_string)?
        .set_min_connections(0)
        .set_max_connections(max_connections)
        .set_connection_increment(1))
}

/// Returns the session id and serial number identifying a session.
fn session(
    conn: &oracledb::Connection,
) -> Result<(usize, usize), oracledb::Error> {
    Ok((conn.session_id()?, conn.serial_num()?))
}

/// Waits up to ten seconds for the given session to end in the database.
fn wait_for_logoff(
    admin_conn: &oracledb::Connection,
    (session_id, serial_num): (usize, usize),
) -> Result<(), oracledb::Error> {
    let started = Instant::now();
    loop {
        let row = admin_conn.query_row(
            "select count(*) from v$session where sid = :1 and serial# = :2",
            &[&(session_id as i64), &(serial_num as i64)],
        )?;
        let count: i64 = row.get(0)?;
        if count == 0 {
            return Ok(());
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the dropped session is still logged on"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
/// Tests that a connection dropped from a pool is never handed out again: it
/// is logged off, the busy and open counts no longer include it, and the pool
/// creates a new connection in its place up to its maximum.
fn test_4600() -> Result<(), oracledb::Error> {
    let admin_conn = common::admin_conn();
    let mut pool = oracledb::create_pool(pool_config(1)?)?;
    let mut first = pool.acquire()?;
    let dropped = session(&first)?;
    first.close_and_drop()?;
    assert_eq!(pool.busy_count()?, 0);
    assert_eq!(pool.open_count()?, 0);
    wait_for_logoff(&admin_conn, dropped)?;

    let mut second = pool.acquire()?;
    assert_ne!(session(&second)?, dropped);
    second.ping()?;
    assert_eq!(pool.busy_count()?, 1);
    assert_eq!(pool.open_count()?, 1);
    second.close()?;
    pool.close()?;
    Ok(())
}

#[test]
/// Tests that a connection whose session was killed without IMMEDIATE, so
/// that its transport stays open, can be dropped from the pool instead of
/// going back to it.
fn test_4601() -> Result<(), oracledb::Error> {
    let admin_conn = common::admin_conn();
    let mut pool = oracledb::create_pool(pool_config(1)?)?;
    // A second loan: the first one ends with a rollback of its own, which
    // fails on a killed session and drops it anyway.
    pool.acquire()?.close()?;
    let mut first = pool.acquire()?;
    let killed = session(&first)?;
    admin_conn.execute(
        &format!("alter system kill session '{},{}'", killed.0, killed.1),
        &[],
    )?;
    let error = first.ping().unwrap_err();
    assert_eq!(error.db_error_code(), Some(28));
    first.close_and_drop()?;
    assert_eq!(pool.busy_count()?, 0);

    let mut second = pool.acquire()?;
    assert_ne!(session(&second)?, killed);
    second.ping()?;
    second.close()?;
    pool.close()?;
    Ok(())
}

#[test]
/// Tests that dropping a connection from a pool at its maximum lets a caller
/// already waiting in acquire() get a new connection.
fn test_4602() -> Result<(), oracledb::Error> {
    let mut pool = oracledb::create_pool(pool_config(1)?)?;
    let mut first = pool.acquire()?;
    let dropped = session(&first)?;
    std::thread::scope(|scope| -> Result<(), oracledb::Error> {
        let waiter =
            scope.spawn(|| -> Result<(usize, usize), oracledb::Error> {
                let mut conn = pool.acquire()?;
                conn.ping()?;
                let id = session(&conn)?;
                conn.close()?;
                Ok(id)
            });
        std::thread::sleep(Duration::from_millis(500));
        assert!(!waiter.is_finished(), "the pool grew beyond its maximum");
        first.close_and_drop()?;
        assert_ne!(waiter.join().unwrap()?, dropped);
        Ok(())
    })?;
    assert_eq!(pool.busy_count()?, 0);
    pool.close()?;
    Ok(())
}

#[test]
/// Tests that close_and_drop() closes a standalone connection, and that a
/// closed connection cannot be closed or dropped again.
fn test_4603() -> Result<(), oracledb::Error> {
    let mut conn = common::conn();
    conn.close_and_drop()?;
    assert!(conn.ping().is_err());
    assert!(conn.close_and_drop().is_err());

    let mut pool = oracledb::create_pool(pool_config(1)?)?;
    let mut pooled = pool.acquire()?;
    pooled.close_and_drop()?;
    assert!(pooled.ping().is_err());
    assert!(pooled.close_and_drop().is_err());
    assert!(pooled.close().is_err());
    pool.close()?;
    Ok(())
}
