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
// test_4300_clob_surrogate_pairs()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;
use std::io::{ErrorKind, Read, Seek, Write};

/// Text mixing 1-, 2-, 3- and 4-byte UTF-8 characters; each 4-byte character
/// is a surrogate pair (two UCS-2 units) in the LOB.
const TEXT: &str = "a😀bç😀😀€d😀😀😀e";

/// Creates a temporary LOB of the given type holding the given text.
fn temp_lob(
    conn: &oracledb::Connection,
    db_type: &'static oracledb::DbType,
    text: &str,
) -> Result<oracledb::Lob, oracledb::Error> {
    let mut lob = conn.create_lob(db_type)?;
    lob.write_all(text.as_bytes())?;
    lob.rewind()?;
    Ok(lob)
}

/// Reads the LOB to its end with the given buffer size, checking that every
/// read returns complete UTF-8.
fn read_all(lob: &mut oracledb::Lob, buf_size: usize) -> String {
    let mut buf = vec![0; buf_size];
    let mut text = String::new();
    loop {
        let num_bytes = lob.read(&mut buf).unwrap_or_else(|error| {
            panic!("read with a {buf_size}-byte buffer failed: {error}")
        });
        if num_bytes == 0 {
            break;
        }
        text.push_str(std::str::from_utf8(&buf[..num_bytes]).unwrap());
    }
    text
}

#[rstest]
/// Tests that CLOB and NCLOB reads succeed with every buffer size from 4
/// bytes up, so that every read boundary falls in turn on each position of
/// the text, including between the two units of a surrogate pair.
fn test_4300(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let num_units = TEXT.encode_utf16().count();
    for db_type in [oracledb::DB_TYPE_CLOB, oracledb::DB_TYPE_NCLOB] {
        let mut lob = temp_lob(&conn, db_type, TEXT)?;
        assert_eq!(lob.get_size()?, num_units);
        for buf_size in 4..=3 * num_units + 3 {
            lob.rewind()?;
            assert_eq!(read_all(&mut lob, buf_size), TEXT, "{buf_size}");
        }
    }
    Ok(())
}

#[rstest]
/// Tests that a CLOB of 200,000 supplementary characters stored in a table
/// is read back exactly with a 64 KiB buffer (an odd number of UCS-2 units
/// per read) and with read_to_string().
fn test_4301(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_4301", "data clob")?;
    let text = "😀".repeat(200_000);
    conn.execute("insert into test_4301 values (empty_clob())", &[])?;
    let mut row = conn
        .statement("select data from test_4301")?
        .fetch_lobs()
        .build()?
        .query_row(&[])?;
    let mut lob: oracledb::Lob = row.take(0)?;
    for chunk in text.as_bytes().chunks(4 * 10_000) {
        lob.write_all(chunk)?;
    }
    let mut row = conn
        .statement("select data from test_4301")?
        .fetch_lobs()
        .build()?
        .query_row(&[])?;
    let mut lob: oracledb::Lob = row.take(0)?;
    assert_eq!(lob.get_size()?, 400_000);
    assert_eq!(read_all(&mut lob, 64 * 1024), text);
    lob.rewind()?;
    let mut read_back = String::new();
    lob.read_to_string(&mut read_back)?;
    assert_eq!(read_back, text);
    Ok(())
}

#[rstest]
/// Tests that a read with a buffer too small for the supplementary character
/// at the current position fails without moving the position.
fn test_4302(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let mut lob = temp_lob(&conn, oracledb::DB_TYPE_CLOB, "a😀b")?;
    let mut buf = [0; 3];
    assert_eq!(lob.read(&mut buf)?, 1);
    assert_eq!(&buf[..1], b"a");
    let error = lob.read(&mut buf).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    let mut buf = [0; 4];
    assert_eq!(lob.read(&mut buf)?, 4);
    assert_eq!(&buf, "😀".as_bytes());
    assert_eq!(lob.read(&mut buf)?, 1);
    assert_eq!(&buf[..1], b"b");
    assert_eq!(lob.read(&mut buf)?, 0);
    Ok(())
}
