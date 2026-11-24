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
// test_4100_session_time_zone()
//-----------------------------------------------------------------------------

use rstest::*;

/// Returns a configuration for a connection whose session time zone is set
/// to the given value when the session is created.
fn config_with_time_zone(time_zone: &str) -> oracledb::Config {
    oracledb::get_test_config().config.clone().set_time_zone(time_zone)
}

#[test]
/// test a session created with a time zone name reports it and follows
/// daylight saving time
fn test_4100() -> Result<(), oracledb::Error> {
    let conn = oracledb::connect(config_with_time_zone("America/New_York"))?;
    let row = conn.query_row(
        "select
            sessiontimezone,
            to_char(current_timestamp, 'TZR'),
            to_char(cast(timestamp '2026-01-15 12:00:00'
                as timestamp with time zone), 'TZH:TZM'),
            to_char(cast(timestamp '2026-07-15 12:00:00'
                as timestamp with time zone), 'TZH:TZM'),
            cast(timestamp '2026-07-15 12:00:00' as timestamp with time zone)
         from dual",
        &[],
    )?;
    let session_time_zone: String = row.get(0)?;
    let current_region: String = row.get(1)?;
    let winter_offset: String = row.get(2)?;
    let summer_offset: String = row.get(3)?;
    let summer: oracledb::OracleTimestamp = row.get(4)?;
    assert_eq!(session_time_zone, "America/New_York");
    assert_eq!(current_region.to_uppercase(), "AMERICA/NEW_YORK");
    assert_eq!(winter_offset, "-05:00");
    assert_eq!(summer_offset, "-04:00");
    assert_eq!(
        summer,
        oracledb::OracleTimestamp::new_utc_timestamp(2026, 7, 15, 16, 0, 0, 0)
    );
    Ok(())
}

#[test]
/// test the session time zone accepts the values that ORA_SDTZ accepts
fn test_4101() -> Result<(), oracledb::Error> {
    let conn = oracledb::connect(config_with_time_zone("-03:00"))?;
    let row = conn.query_row("select sessiontimezone from dual", &[])?;
    let session_time_zone: String = row.get(0)?;
    assert_eq!(session_time_zone, "-03:00");

    let conn = oracledb::connect(config_with_time_zone("dbtimezone"))?;
    let row =
        conn.query_row("select sessiontimezone, dbtimezone from dual", &[])?;
    let session_time_zone: String = row.get(0)?;
    let db_time_zone: String = row.get(1)?;
    assert_eq!(session_time_zone, db_time_zone);
    Ok(())
}

#[rstest]
#[case("Not/A_Zone")]
#[case("UTC', nls_language = 'GERMAN")]
/// test an unknown session time zone fails the connection
fn test_4102(#[case] time_zone: &str) {
    let error = oracledb::connect(config_with_time_zone(time_zone))
        .expect_err("connection with an unknown time zone succeeded");
    assert!(error.db_error_code().is_some(), "unexpected error: {error}");
}

#[test]
/// test the connections of a pool get the session time zone
fn test_4103() -> Result<(), oracledb::Error> {
    let test_config = oracledb::get_test_config();
    let config = oracledb::PoolConfig::default()
        .set_credentials(&test_config.user, &test_config.password)
        .set_connect_string(&test_config.connect_string)?
        .set_time_zone("Asia/Kathmandu")
        .set_min_connections(0)
        .set_max_connections(1);
    assert_eq!(config.time_zone(), Some("Asia/Kathmandu"));
    let mut pool = oracledb::create_pool(config)?;
    let conn = pool.acquire()?;
    let row = conn.query_row("select sessiontimezone from dual", &[])?;
    let session_time_zone: String = row.get(0)?;
    assert_eq!(session_time_zone, "Asia/Kathmandu");
    drop(conn);
    pool.close()?;
    Ok(())
}
