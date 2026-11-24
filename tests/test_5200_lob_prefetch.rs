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
// test_5200_lob_prefetch()
//-----------------------------------------------------------------------------

mod common;

use common::{admin_conn, conn};
use rstest::*;
use std::io::{Read, Seek, Write};

/// Text mixing 1-, 2-, 3- and 4-byte UTF-8 characters: 18 UCS-2 units, as
/// each 4-byte character is a surrogate pair.
const TEXT: &str = "Ação 日本 😀 b😀😀 €";

/// The LOB prefetch size used by these tests.
const PREFETCH: u32 = 1000;

/// Returns the session id of the connection.
fn sid(conn: &oracledb::Connection) -> Result<String, oracledb::Error> {
    conn.query_row("select sys_context('USERENV', 'SID') from dual", &[])?
        .get(0)
}

/// Returns the number of round trips the session has made so far, as seen
/// from the admin connection (which costs the session no round trip).
fn round_trips(
    admin_conn: &oracledb::Connection,
    sid: &str,
) -> Result<i64, oracledb::Error> {
    admin_conn
        .query_row(
            r#"
            select s.value
            from v$sesstat s
                join v$statname n on n.statistic# = s.statistic#
            where s.sid = :1
              and n.name = 'SQL*Net roundtrips to/from client'
            "#,
            &[&sid],
        )?
        .get(0)
}

/// Runs the closure and returns its result with the number of round trips
/// the session made while it ran.
fn during<T>(
    admin_conn: &oracledb::Connection,
    sid: &str,
    f: impl FnOnce() -> Result<T, oracledb::Error>,
) -> Result<(T, i64), oracledb::Error> {
    let before = round_trips(admin_conn, sid)?;
    let value = f()?;
    Ok((value, round_trips(admin_conn, sid)? - before))
}

/// Reads the LOB from its current position to its end with the given buffer
/// size.
fn read_with_buffer(
    lob: &mut oracledb::Lob,
    buf_size: usize,
) -> Result<Vec<u8>, oracledb::Error> {
    let mut buf = vec![0; buf_size];
    let mut data = Vec::new();
    loop {
        let num_bytes = lob.read(&mut buf)?;
        if num_bytes == 0 {
            break;
        }
        data.extend_from_slice(&buf[..num_bytes]);
    }
    Ok(data)
}

/// Reads a CLOB or NCLOB to its end with a buffer large enough for the whole
/// text, so that no read ends inside a surrogate pair.
fn read_text(lob: &mut oracledb::Lob) -> Result<String, oracledb::Error> {
    let buf_size = 3 * lob.get_size()? + 3;
    Ok(String::from_utf8(read_with_buffer(lob, buf_size)?).unwrap())
}

/// Reads a BLOB to its end.
fn read_bytes(lob: &mut oracledb::Lob) -> Result<Vec<u8>, oracledb::Error> {
    let buf_size = lob.get_size()? + 1;
    read_with_buffer(lob, buf_size)
}

/// Creates the table used by the tests, with one row per given text: the
/// text as CLOB and NCLOB, and its UTF-8 bytes as BLOB.
fn create_table<'a>(
    conn: &'a oracledb::Connection,
    name: &'a str,
    texts: &[&str],
) -> Result<common::TableGuard<'a>, oracledb::Error> {
    let guard = common::create_table(
        conn,
        name,
        "id number(9), c clob, n nclob, b blob",
    )?;
    for (i, text) in texts.iter().enumerate() {
        let id = i as i32 + 1;
        let bytes = text.as_bytes().to_vec();
        conn.execute(
            &format!("insert into {name} values (:1, :2, :3, :4)"),
            &[&id, text, text, &bytes],
        )?;
    }
    conn.commit()?;
    Ok(guard)
}

#[rstest]
/// Tests that CLOB, NCLOB and BLOB values within the LOB prefetch size arrive
/// with their locators: reading them costs no round trip.
fn test_5200(
    conn: oracledb::Connection,
    admin_conn: oracledb::Connection,
) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn, "test_5200", &[TEXT])?;
    let sid = sid(&conn)?;
    let mut row = conn
        .statement("select c, n, b from test_5200")?
        .fetch_lobs()
        .lob_prefetch_size(PREFETCH)
        .build()?
        .query_row(&[])?;
    let mut clob: oracledb::Lob = row.take(0)?;
    let mut nclob: oracledb::Lob = row.take(1)?;
    let mut blob: oracledb::Lob = row.take(2)?;
    let num_units = TEXT.encode_utf16().count();
    let (values, trips) = during(&admin_conn, &sid, || {
        Ok((
            clob.get_size()?,
            read_text(&mut clob)?,
            nclob.get_size()?,
            read_text(&mut nclob)?,
            blob.get_size()?,
            read_bytes(&mut blob)?,
        ))
    })?;
    assert_eq!(values.0, num_units);
    assert_eq!(values.1, TEXT);
    assert_eq!(values.2, num_units);
    assert_eq!(values.3, TEXT);
    assert_eq!(values.4, TEXT.len());
    assert_eq!(values.5, TEXT.as_bytes());
    assert_eq!(trips, 0, "round trips of the reads");
    Ok(())
}

#[rstest]
/// Tests that a LOB larger than the LOB prefetch size, with the end of the
/// prefetched data on the first unit of a surrogate pair, reads back exactly:
/// a read within the prefetched data costs no round trip, a read past it
/// goes to the server.
fn test_5201(
    conn: oracledb::Connection,
    admin_conn: oracledb::Connection,
) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn, "test_5201", &[TEXT])?;
    let sid = sid(&conn)?;
    // "Ação 日本 " is 8 units: 9 units end on the first unit of "😀".
    let prefetch = 9;
    for column in ["c", "n"] {
        let mut row = conn
            .statement(&format!("select {column} from test_5201"))?
            .fetch_lobs()
            .lob_prefetch_size(prefetch)
            .build()?
            .query_row(&[])?;
        let mut lob: oracledb::Lob = row.take(0)?;
        assert_eq!(lob.get_size()?, TEXT.encode_utf16().count());
        // 12 bytes ask for 4 units.
        let (head, trips) =
            during(&admin_conn, &sid, || read_with_buffer_once(&mut lob, 12))?;
        assert_eq!(head, "Ação".as_bytes(), "{column}");
        assert_eq!(trips, 0, "{column}: a read within the prefetched data");
        let (tail, trips) = during(&admin_conn, &sid, || read_text(&mut lob))?;
        assert_eq!(tail, &TEXT[TEXT.find(' ').unwrap()..], "{column}");
        assert!(trips >= 1, "{column}: a read past the prefetched data");
        lob.rewind()?;
        assert_eq!(read_text(&mut lob)?, TEXT, "{column}");
    }
    let mut row = conn
        .statement("select b from test_5201")?
        .fetch_lobs()
        .lob_prefetch_size(prefetch)
        .build()?
        .query_row(&[])?;
    let mut lob: oracledb::Lob = row.take(0)?;
    assert_eq!(read_bytes(&mut lob)?, TEXT.as_bytes());
    Ok(())
}

/// Makes a single read with the given buffer size.
fn read_with_buffer_once(
    lob: &mut oracledb::Lob,
    buf_size: usize,
) -> Result<Vec<u8>, oracledb::Error> {
    let mut buf = vec![0; buf_size];
    let num_bytes = lob.read(&mut buf)?;
    Ok(buf[..num_bytes].to_vec())
}

#[rstest]
/// Tests that the rows of later fetches, and of a later execution of the
/// cached statement, also arrive with their LOB data; that empty LOBs read
/// as empty and NULL stays NULL.
fn test_5202(
    conn: oracledb::Connection,
    admin_conn: oracledb::Connection,
) -> Result<(), oracledb::Error> {
    let texts: Vec<String> = (1..=7).map(|i| format!("{i} {TEXT}")).collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    let _guard = create_table(&conn, "test_5202", &texts)?;
    conn.execute("insert into test_5202 values (8, empty_clob(), empty_clob(), empty_blob())", &[])?;
    conn.execute("insert into test_5202 values (9, null, null, null)", &[])?;
    let sid = sid(&conn)?;
    for _ in 0..2 {
        let rows = conn
            .statement("select c, n, b from test_5202 order by id")?
            .fetch_lobs()
            .lob_prefetch_size(PREFETCH)
            .prefetch_rows(2)
            .fetch_array_size(3)
            .build()?
            .query(&[])?;
        let mut count = 0;
        for row in rows {
            let mut row = row?;
            let clob: Option<oracledb::Lob> = row.take(0)?;
            let nclob: Option<oracledb::Lob> = row.take(1)?;
            let blob: Option<oracledb::Lob> = row.take(2)?;
            count += 1;
            if count == 9 {
                assert!(clob.is_none() && nclob.is_none() && blob.is_none());
                continue;
            }
            let expected = texts.get(count - 1).copied().unwrap_or("");
            let (mut clob, mut nclob, mut blob) =
                (clob.unwrap(), nclob.unwrap(), blob.unwrap());
            let (values, trips) = during(&admin_conn, &sid, || {
                Ok((
                    read_text(&mut clob)?,
                    read_text(&mut nclob)?,
                    read_bytes(&mut blob)?,
                ))
            })?;
            assert_eq!(values.0, expected, "row {count}");
            assert_eq!(values.1, expected, "row {count}");
            assert_eq!(values.2, expected.as_bytes(), "row {count}");
            assert_eq!(trips, 0, "round trips of the reads of row {count}");
        }
        assert_eq!(count, 9);
    }
    Ok(())
}

#[rstest]
/// Tests that without a LOB prefetch size reading a LOB still costs a round
/// trip, and that setting one on a statement already cached takes effect.
fn test_5203(
    conn: oracledb::Connection,
    admin_conn: oracledb::Connection,
) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn, "test_5203", &[TEXT])?;
    let sid = sid(&conn)?;
    for (prefetch, expected_trips) in
        [(None, 1), (Some(PREFETCH), 0), (None, 1)]
    {
        let mut builder =
            conn.statement("select c from test_5203")?.fetch_lobs();
        if let Some(size) = prefetch {
            builder = builder.lob_prefetch_size(size);
        }
        let mut row = builder.build()?.query_row(&[])?;
        let mut lob: oracledb::Lob = row.take(0)?;
        let (text, trips) = during(&admin_conn, &sid, || read_text(&mut lob))?;
        assert_eq!(text, TEXT);
        assert_eq!(trips, expected_trips, "{prefetch:?}");
    }
    Ok(())
}

#[rstest]
/// Tests that a prefetched LOB that is written to or trimmed reads back its
/// new value, not the prefetched one.
fn test_5204(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = create_table(&conn, "test_5204", &[TEXT])?;
    let mut row = conn
        .statement("select c, b from test_5204 for update")?
        .fetch_lobs()
        .lob_prefetch_size(PREFETCH)
        .build()?
        .query_row(&[])?;
    let mut clob: oracledb::Lob = row.take(0)?;
    let mut blob: oracledb::Lob = row.take(1)?;
    clob.write_all("Z".as_bytes())?;
    clob.rewind()?;
    assert_eq!(read_text(&mut clob)?, format!("Z{}", &TEXT[1..]));
    blob.write_all(b"Z")?;
    blob.rewind()?;
    assert_eq!(read_bytes(&mut blob)?, format!("Z{}", &TEXT[1..]).as_bytes());
    clob.trim(4)?;
    clob.rewind()?;
    assert_eq!(read_text(&mut clob)?, "Zção");
    blob.trim(2)?;
    blob.rewind()?;
    assert_eq!(read_bytes(&mut blob)?, b"Z\xc3");
    conn.rollback()?;
    Ok(())
}
