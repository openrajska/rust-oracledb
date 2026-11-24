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
// test_5700_large_responses()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

/// The size of the LOBs read in one call: a response of a few hundred
/// packets at the default SDU.
const SIZE: usize = 4 * 1024 * 1024;

/// The TTC message type of the end of a response.
const END_OF_RESPONSE: u8 = 29;

/// The TTC message type of a renegotiation request.
const RENEGOTIATE: u8 = 28;

/// Creates a temporary BLOB holding the given data, positioned at its start.
fn temp_blob(
    conn: &oracledb::Connection,
    data: &[u8],
) -> Result<oracledb::Lob, oracledb::Error> {
    let mut lob = conn.create_lob(oracledb::DB_TYPE_BLOB)?;
    lob.write_all(data)?;
    lob.seek(SeekFrom::Start(1))?;
    Ok(lob)
}

/// Reads the whole LOB in one call and returns the data with the time the
/// read took.
fn read_whole(
    lob: &mut oracledb::Lob,
) -> Result<(Vec<u8>, Duration), oracledb::Error> {
    let mut buf = vec![0; lob.get_size()?];
    let start = Instant::now();
    let num_bytes = lob.read(&mut buf)?;
    let elapsed = start.elapsed();
    buf.truncate(num_bytes);
    Ok((buf, elapsed))
}

/// Returns the shortest of three reads of a BLOB of `SIZE` bytes, all equal
/// to `byte`, checking that each read returns the whole value.
fn fastest_read(
    conn: &oracledb::Connection,
    byte: u8,
) -> Result<Duration, oracledb::Error> {
    let data = vec![byte; SIZE];
    let mut lob = temp_blob(conn, &data)?;
    let mut fastest = Duration::MAX;
    for _ in 0..3 {
        lob.seek(SeekFrom::Start(1))?;
        let (read, elapsed) = read_whole(&mut lob)?;
        assert_eq!(read.len(), data.len());
        assert!(read == data, "the BLOB of {byte:#04x} bytes reads back");
        fastest = fastest.min(elapsed);
    }
    Ok(fastest)
}

/// Checks that a BLOB whose bytes are all `byte` reads about as fast as one
/// of zero bytes: a data packet that only looks like the end of the
/// response must not make the client assemble and parse the response again.
fn reads_as_fast_as_zeros(
    conn: &oracledb::Connection,
    byte: u8,
) -> Result<(), oracledb::Error> {
    let zeros = fastest_read(conn, 0)?;
    let other = fastest_read(conn, byte)?;
    assert!(
        other <= zeros * 3 + Duration::from_millis(10),
        "a BLOB of {byte:#04x} bytes read in {other:?}, one of zero bytes in {zeros:?}"
    );
    Ok(())
}

#[rstest]
/// A response whose data packets end with the end-of-response message type
/// is received once.
fn test_5700(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    reads_as_fast_as_zeros(&conn, END_OF_RESPONSE)
}

#[rstest]
/// A response whose data packets start with the renegotiation message type
/// is received once.
fn test_5701(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    reads_as_fast_as_zeros(&conn, RENEGOTIATE)
}

#[rstest]
/// A large BLOB and CLOB read in one call each return exactly their data.
fn test_5702(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
    let mut blob = temp_blob(&conn, &data)?;
    let (read, _) = read_whole(&mut blob)?;
    assert!(read == data, "the BLOB reads back");

    let text = "Ação 日本 😀 €\n".repeat(SIZE / 32);
    let mut clob = conn.create_lob(oracledb::DB_TYPE_CLOB)?;
    clob.write_all(text.as_bytes())?;
    clob.seek(SeekFrom::Start(1))?;
    let mut buf = vec![0; 3 * clob.get_size()? + 3];
    let num_bytes = clob.read(&mut buf)?;
    assert_eq!(std::str::from_utf8(&buf[..num_bytes]).unwrap(), text);
    Ok(())
}
