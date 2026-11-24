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
// test_4000_timestamp_tz()
//-----------------------------------------------------------------------------

use std::collections::HashMap;

mod common;

use common::conn;
use rstest::*;

/// The format used to send and render TIMESTAMP WITH TIME ZONE values as text
/// with their offset.
const TSTZ_FORMAT: &str = "YYYY-MM-DD HH24:MI:SS.FF3 TZH:TZM";

/// Returns the timestamp with a time zone whose UTC date and time are given
/// (with 123 milliseconds) and which keeps the given offset.
fn tstz(
    utc: (i16, u8, u8, u8, u8, u8),
    tz_hour_offset: i8,
    tz_minute_offset: i8,
) -> oracledb::OracleTimestamp {
    let (year, month, day, hour, minute, second) = utc;
    oracledb::OracleTimestamp::new_utc_timestamp(
        year,
        month,
        day,
        hour,
        minute,
        second,
        123_000_000,
    )
    .with_tz_offset(tz_hour_offset, tz_minute_offset)
    .unwrap()
}

#[rstest]
#[case("2026-03-01 10:00:00.123 -03:00", (2026, 3, 1, 13, 0, 0), -3, 0)]
#[case("2026-03-01 10:00:00.123 +05:45", (2026, 3, 1, 4, 15, 0), 5, 45)]
#[case("2026-03-01 10:00:00.123 +00:00", (2026, 3, 1, 10, 0, 0), 0, 0)]
#[case("2026-01-01 01:00:00.123 +05:45", (2025, 12, 31, 19, 15, 0), 5, 45)]
#[case("2026-03-01 10:00:00.123 -00:30", (2026, 3, 1, 10, 30, 0), 0, -30)]
#[case("2026-03-01 10:00:00.123 -09:30", (2026, 3, 1, 19, 30, 0), -9, -30)]
/// test fetching TIMESTAMP WITH TIME ZONE values keeps their offset
fn test_4000(
    conn: oracledb::Connection,
    #[case] text: &str,
    #[case] utc: (i16, u8, u8, u8, u8, u8),
    #[case] tz_hour_offset: i8,
    #[case] tz_minute_offset: i8,
) -> Result<(), oracledb::Error> {
    let row = conn.query_row(
        &format!("select to_timestamp_tz(:1, '{TSTZ_FORMAT}') from dual"),
        &[&text],
    )?;
    let value: oracledb::OracleTimestamp = row.get(0)?;
    assert_eq!(value, tstz(utc, tz_hour_offset, tz_minute_offset));
    assert_eq!(value.tz_hour_offset(), tz_hour_offset);
    assert_eq!(value.tz_minute_offset(), tz_minute_offset);
    assert!(value.is_utc());
    Ok(())
}

#[rstest]
#[case((2026, 3, 1, 13, 0, 0), -3, 0, "2026-03-01 10:00:00.123 -03:00")]
#[case((2026, 3, 1, 4, 15, 0), 5, 45, "2026-03-01 10:00:00.123 +05:45")]
#[case((2026, 3, 1, 10, 0, 0), 0, 0, "2026-03-01 10:00:00.123 +00:00")]
#[case((2025, 12, 31, 19, 15, 0), 5, 45, "2026-01-01 01:00:00.123 +05:45")]
#[case((2026, 3, 1, 10, 30, 0), 0, -30, "2026-03-01 10:00:00.123 -00:30")]
/// test binding TIMESTAMP WITH TIME ZONE values sends their offset
fn test_4001(
    conn: oracledb::Connection,
    #[case] utc: (i16, u8, u8, u8, u8, u8),
    #[case] tz_hour_offset: i8,
    #[case] tz_minute_offset: i8,
    #[case] text: &str,
) -> Result<(), oracledb::Error> {
    let value = tstz(utc, tz_hour_offset, tz_minute_offset);
    let row = conn.query_row(
        &format!("select to_char(:1, '{TSTZ_FORMAT}'), :2 from dual"),
        &[&value, &value],
    )?;
    let fetched_text: String = row.get(0)?;
    let fetched_value: oracledb::OracleTimestamp = row.get(1)?;
    assert_eq!(fetched_text, text);
    assert_eq!(fetched_value, value);
    Ok(())
}

#[rstest]
/// test storing TIMESTAMP WITH TIME ZONE values in a table keeps their offset
fn test_4002(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(
        &conn,
        "TestTimestampTzOffsets",
        "id number(9), value timestamp(3) with time zone",
    )?;
    let values = [
        tstz((2026, 3, 1, 13, 0, 0), -3, 0),
        tstz((2026, 3, 1, 4, 15, 0), 5, 45),
        tstz((2026, 3, 1, 10, 0, 0), 0, 0),
    ];
    for (id, value) in values.iter().enumerate() {
        conn.execute(
            "insert into TestTimestampTzOffsets values (:1, :2)",
            &[&id, value],
        )?;
    }
    let cursor = conn.query(
        &format!(
            "select value, to_char(value, '{TSTZ_FORMAT}')
             from TestTimestampTzOffsets order by id"
        ),
        &[],
    )?;
    let mut fetched = Vec::new();
    for row in cursor {
        let row = row?;
        let value: oracledb::OracleTimestamp = row.get(0)?;
        let text: String = row.get(1)?;
        fetched.push((value, text));
    }
    assert_eq!(
        fetched,
        vec![
            (values[0], "2026-03-01 10:00:00.123 -03:00".to_string()),
            (values[1], "2026-03-01 10:00:00.123 +05:45".to_string()),
            (values[2], "2026-03-01 10:00:00.123 +00:00".to_string()),
        ]
    );
    Ok(())
}

#[rstest]
#[case("2026-07-01 12:00:00.123 America/New_York", (2026, 7, 1, 16, 0, 0))]
#[case("2026-01-01 12:00:00.123 America/New_York", (2026, 1, 1, 17, 0, 0))]
#[case("2026-03-01 10:00:00.123 Asia/Kathmandu", (2026, 3, 1, 4, 15, 0))]
/// test fetching TIMESTAMP WITH TIME ZONE values stored with a time zone
/// region returns the instant in UTC
fn test_4003(
    conn: oracledb::Connection,
    #[case] text: &str,
    #[case] utc: (i16, u8, u8, u8, u8, u8),
) -> Result<(), oracledb::Error> {
    let row = conn.query_row(
        "select to_timestamp_tz(:1, 'YYYY-MM-DD HH24:MI:SS.FF3 TZR')
         from dual",
        &[&text],
    )?;
    let value: oracledb::OracleTimestamp = row.get(0)?;
    assert_eq!(value, tstz(utc, 0, 0));
    assert!(value.is_utc());
    Ok(())
}

#[rstest]
/// test fetching and binding JSON with TIMESTAMP WITH TIME ZONE values keeps
/// their offset
fn test_4004(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    if common::skip_unless_native_json_supported(&conn) {
        return Ok(());
    }
    let row = conn.query_row(
        &format!(
            "select json_object(
                'minus' value to_timestamp_tz(:1, '{TSTZ_FORMAT}'),
                'plus' value to_timestamp_tz(:2, '{TSTZ_FORMAT}')
                returning json)
             from dual"
        ),
        &[
            &"2026-03-01 10:00:00.123 -03:00",
            &"2026-03-01 10:00:00.123 +05:45",
        ],
    )?;
    let fetched: oracledb::JsonValue = row.get(0)?;
    let minus = tstz((2026, 3, 1, 13, 0, 0), -3, 0);
    let plus = tstz((2026, 3, 1, 4, 15, 0), 5, 45);
    if let oracledb::JsonValue::JsonObject(map) = &fetched {
        assert!(matches!(
            map.get("minus"),
            Some(oracledb::JsonValue::Timestamp(v)) if *v == minus
        ));
        assert!(matches!(
            map.get("plus"),
            Some(oracledb::JsonValue::Timestamp(v)) if *v == plus
        ));
    } else {
        panic!("expected a JSON object, got {fetched:?}");
    }

    let mut map = HashMap::new();
    map.insert("minus".to_string(), oracledb::JsonValue::Timestamp(minus));
    map.insert("plus".to_string(), oracledb::JsonValue::Timestamp(plus));
    let value = oracledb::JsonValue::JsonObject(map);
    let row = conn.query_row(
        "select json_value(:1, '$.minus' returning varchar2(40)),
                json_value(:2, '$.plus' returning varchar2(40))
         from dual",
        &[&value, &value],
    )?;
    let minus_text: String = row.get(0)?;
    let plus_text: String = row.get(1)?;
    assert_eq!(minus_text, "2026-03-01T10:00:00.123000-03:00");
    assert_eq!(plus_text, "2026-03-01T10:00:00.123000+05:45");
    Ok(())
}

#[test]
/// test the time zone offset of timestamps built on the client
fn test_4005() {
    let utc =
        oracledb::OracleTimestamp::new_utc_timestamp(2026, 3, 1, 13, 0, 0, 0);
    assert_eq!(utc.tz_hour_offset(), 0);
    assert_eq!(utc.tz_minute_offset(), 0);
    let value = utc.with_tz_offset(-3, -30).unwrap();
    assert!(value.is_utc());
    assert_eq!(value.hour(), 13);
    assert_eq!(value.tz_hour_offset(), -3);
    assert_eq!(value.tz_minute_offset(), -30);
    assert_ne!(value, utc);
    let naive =
        oracledb::OracleTimestamp::new_timestamp(2026, 3, 1, 13, 0, 0, 0)
            .with_tz_offset(1, 0)
            .unwrap();
    assert!(naive.is_utc());
    assert_eq!(naive, utc.with_tz_offset(1, 0).unwrap());
    for (hours, minutes) in
        [(16, 0), (-16, 0), (0, 60), (0, -60), (-3, 30), (3, -30)]
    {
        assert!(
            utc.with_tz_offset(hours, minutes).is_err(),
            "offset {hours}:{minutes} accepted"
        );
    }
}
