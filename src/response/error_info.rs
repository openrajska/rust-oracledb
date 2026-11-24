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
// error_info.rs
//
// Defines the structure representing error information returned by the server.
//-----------------------------------------------------------------------------

use crate::client::Client;
use crate::constants;
use crate::error::Error;
use crate::response::Response;
use crate::rowid::Rowid;

pub(crate) struct ErrorInfo {
    db_error: Option<DbError>,
    cursor_id: u16,
    flags: u8,
    rowcount: u64,
    batch_errors: Vec<DbError>,
    num_batch_errors: usize,
}

impl ErrorInfo {
    /// Deserializes the error information from the database response.
    pub fn deserialize(
        resp: &mut Response,
        client: &Client,
    ) -> Result<ErrorInfo, Error> {
        resp.deserialize_status()?;
        let _current_row_num = resp.read_ub4()?;
        let _error_num_short = resp.read_ub2()?;
        let _array_elem_error_1 = resp.read_ub2()?;
        let _array_elem_error_2 = resp.read_ub2()?;
        let cursor_id = resp.read_ub2()?;
        let error_pos = resp.read_ub2()?;
        let _old_sql_type = resp.read_u8()?;
        let _fatal = resp.read_u8()?;
        let _flags_1 = resp.read_u8()?;
        let _user_cursor_options = resp.read_u8()?;
        let _upi_parameter = resp.read_u8()?;
        let flags = resp.read_u8()?;
        let _rowid = Rowid::deserialize(resp)?;
        let _os_error = resp.read_ub4()?;
        let _statement_num = resp.read_u8()?;
        let _call_num = resp.read_u8()?;
        let _padding = resp.read_ub2()?;
        let _success_iters = resp.read_ub4()?;
        if resp.read_ub4()? > 0 {
            let _logical_rowid = resp.read_bytes_with_length()?;
        }
        let (batch_errors, num_batch_errors) =
            Self::deserialize_batch_errors(resp)?;
        let error_num = resp.read_ub4()?;
        let rowcount = resp.read_ub8()?;
        if client.supports_ttc_field_version(constants::TTC_FIELD_VERSION_20_1)
        {
            let _sql_type = resp.read_ub4()?;
            let _server_checksum = resp.read_ub4()?;
        }
        let db_error = if error_num == 0 {
            None
        } else {
            let message = resp.read_utf8_with_length()?;
            Some(DbError {
                code: error_num as usize,
                offset: error_pos as usize,
                message: message.trim_end().to_string(),
            })
        };

        Ok(ErrorInfo {
            cursor_id,
            flags,
            rowcount,
            db_error,
            batch_errors,
            num_batch_errors,
        })
    }

    /// Deserializes the errors of the rows of a batch executed with batch
    /// errors enabled: an array of error codes, then an array of row offsets
    /// and then an array of messages, each array empty when no row failed.
    /// An array of codes or offsets whose first byte is the long length
    /// indicator is sent in chunks: each element is preceded by a chunk
    /// length and the array ends with a marker. The number of codes and of
    /// messages is sent in two bytes and the number of offsets in four, so
    /// the number of offsets is the number of failing rows, returned with
    /// the errors.
    fn deserialize_batch_errors(
        resp: &mut Response,
    ) -> Result<(Vec<DbError>, usize), Error> {
        let num_errors = resp.read_ub2()? as usize;
        let mut batch_errors = Vec::with_capacity(num_errors);
        if num_errors > 0 {
            let chunked =
                resp.read_u8()? == constants::TTC_LONG_LENGTH_INDICATOR;
            for _ in 0..num_errors {
                if chunked {
                    resp.read_ub4()?; // chunk length
                }
                batch_errors.push(DbError {
                    code: resp.read_ub2()? as usize,
                    offset: 0,
                    message: String::new(),
                });
            }
            if chunked {
                resp.advance(1)?; // end marker
            }
        }
        let num_offsets = resp.read_ub4()? as usize;
        if num_offsets > 0 {
            let chunked =
                resp.read_u8()? == constants::TTC_LONG_LENGTH_INDICATOR;
            for i in 0..num_offsets {
                if chunked {
                    resp.read_ub4()?; // chunk length
                }
                let offset = resp.read_ub4()? as usize;
                if let Some(batch_error) = batch_errors.get_mut(i) {
                    batch_error.offset = offset;
                }
            }
            if chunked {
                resp.advance(1)?; // end marker
            }
        }
        let num_messages = resp.read_ub2()? as usize;
        if num_messages > 0 {
            resp.advance(1)?; // packed size
            for i in 0..num_messages {
                resp.read_ub2()?; // chunk length
                let message =
                    resp.read_utf8_with_length()?.trim_end().to_string();
                if let Some(batch_error) = batch_errors.get_mut(i) {
                    batch_error.message = message;
                }
                resp.advance(2)?; // end marker
            }
        }
        // the server groups the errors by the check that failed, not by row
        batch_errors.sort_by_key(|e| e.offset);
        Ok((batch_errors, num_offsets))
    }

    /// Returns the cursor id.
    pub(crate) fn cursor_id(&self) -> u16 {
        self.cursor_id
    }

    /// Returns whether or not a compilation warning was returned.
    pub(crate) fn is_compilation_warning(&self) -> bool {
        self.flags & 0x20 != 0
    }

    /// Returns the row count.
    pub(crate) fn rowcount(&self) -> u64 {
        self.rowcount
    }

    /// Returns the number of failing rows of a batch when more failed than the
    /// server can return the errors of, in which case those are incomplete.
    pub(crate) fn too_many_batch_errors(&self) -> Option<usize> {
        (self.num_batch_errors > u16::MAX as usize)
            .then_some(self.num_batch_errors)
    }

    /// Takes the errors of the rows of a batch and returns them.
    pub(crate) fn take_batch_errors(&mut self) -> Vec<DbError> {
        std::mem::take(&mut self.batch_errors)
    }

    /// Takes the error message from the response and returns it.
    pub(crate) fn take_db_error(&mut self) -> Option<DbError> {
        self.db_error.take()
    }

    /// Transfers information from another error info structure that was
    /// received earlier. This is intended for us when a batch of statements is
    /// being executed and a single response is being returned.
    pub(crate) fn transfer_into(&mut self, other_info: &mut ErrorInfo) {
        self.rowcount += other_info.rowcount;
    }
}

/// Represents errors returned by the database.
#[derive(Clone, Debug, PartialEq)]
pub struct DbError {
    code: usize,
    offset: usize,
    message: String,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "{}", self.message)
    }
}

impl DbError {
    /// Returns whether or not a database error should result in the statement
    /// that executed it being closed (and therefore reparsed when another
    /// attempt to execute it takes place). Data integrity errors are known to
    /// be acceptable. All other errors will result in the cursor being closed.
    pub(crate) fn should_close_cursor(&self) -> bool {
        !matches!(
            self.code,
            constants::DB_ERR_NUM_CANNOT_INSERT_NULL
                | constants::DB_ERR_NUM_CHECK_CONSTRAINT_VIOLATED
                | constants::DB_ERR_NUM_CHILD_RECORD_FOUND
                | constants::DB_ERR_NUM_DBOBJECT_CONSTRAINT_VIOLATED
                | constants::DB_ERR_NUM_JSON_SERIALIZER_ERROR
                | constants::DB_ERR_NUM_PARENT_KEY_NOT_FOUND
                | constants::DB_ERR_NUM_VALUE_LARGER_THAN_PRECISION
                | constants::DB_ERR_NUM_UNIQUE_CONSTRAINT_VIOLATED
        )
    }

    /// Returns the error code associated with the database error.
    pub fn code(&self) -> usize {
        self.code
    }

    /// Returns the error message associated with the database error.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the offset associated with the database error: the position
    /// in the SQL text where the error was found or, for an error returned by
    /// [ExecBatchResult::batch_errors()](crate::ExecBatchResult::batch_errors()),
    /// the index of the failing row in the batch, starting from zero.
    pub fn offset(&self) -> usize {
        self.offset
    }
}
