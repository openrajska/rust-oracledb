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
// timestamp.rs
//
// Defines the structure representing Oracle timestamps.
//-----------------------------------------------------------------------------

use chrono::Datelike;
use chrono::Timelike;
use std::fmt;
use std::fmt::Write;

#[cfg(feature = "arrow")]
use arrow_schema::TimeUnit;

use crate::constants;
use crate::db_type::DbType;
use crate::error::Error;
use crate::read_buffer::FromBuf;
use crate::write_buffer::ToBuf;
use crate::write_buffer::WriteBuffer;

/// Represents the value of DATE, TIMESTAMP, TIMESTAMP WITH LOCAL TIME ZONE and
/// TIMESTAMP WITH TIME ZONE columns in the database.
///
/// TIMESTAMP WITH TIME ZONE and TIMESTAMP WITH LOCAL TIME ZONE values are
/// UTC timestamps: their date and time are in UTC. A TIMESTAMP WITH TIME ZONE
/// value also keeps the offset of its time zone (see
/// [`tz_hour_offset()`](Self::tz_hour_offset)), which is sent back to the
/// database when the value is bound. A value stored with a time zone region
/// name instead of an offset is fetched with an offset of zero, since the
/// region's offset at that instant is not known on the client.
#[derive(Clone, Copy, PartialEq)]
pub struct OracleTimestamp {
    year: i16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    nanoseconds: u32,
    is_utc: bool,
    tz_hour_offset: i8,
    tz_minute_offset: i8,
}

impl Default for OracleTimestamp {
    fn default() -> Self {
        Self {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            nanoseconds: 0,
            is_utc: true,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        }
    }
}

impl OracleTimestamp {
    /// Adjusts the timestamp to be UTC based on the database time zone offset.
    /// An error is returned if the database timezone offset is a named time
    /// zone or if the conversion to chrono::NaiveDate is unsuccessful.
    pub(crate) fn adjust_timestamp_ltz_to_utc(
        &mut self,
        db_timezone_offset: chrono::Duration,
    ) -> Result<(), Error> {
        if !db_timezone_offset.is_zero() {
            let date = chrono::NaiveDate::from_ymd_opt(
                self.year.into(),
                self.month.into(),
                self.day.into(),
            )
            .ok_or_else(|| {
                Error::out_of_range(format!(
                    "invalid date: {}-{}-{}",
                    self.year, self.month, self.day
                ))
            })?
            .and_hms_opt(
                self.hour.into(),
                self.minute.into(),
                self.second.into(),
            )
            .ok_or_else(|| {
                Error::out_of_range(format!(
                    "invalid time: {}:{}:{}",
                    self.hour, self.minute, self.second,
                ))
            })?;
            let new_date = date
                .checked_sub_signed(db_timezone_offset)
                .ok_or_else(|| {
                    Error::out_of_range(format!(
                        "overflow adding offset {} to date {}",
                        date, db_timezone_offset
                    ))
                })?;
            self.year = new_date.year().try_into().unwrap();
            self.month = new_date.month().try_into().unwrap();
            self.day = new_date.day().try_into().unwrap();
            self.hour = new_date.hour().try_into().unwrap();
            self.minute = new_date.minute().try_into().unwrap();
        }
        self.is_utc = true;
        Ok(())
    }

    #[cfg(feature = "arrow")]
    /// Returns the timestamp in the specified Arrow time unit.
    pub(crate) fn to_timestamp(
        self,
        time_unit: TimeUnit,
    ) -> Result<i64, Error> {
        let timestamp = chrono::NaiveDateTime::try_from(self)?.and_utc();
        match time_unit {
            TimeUnit::Second => Ok(timestamp.timestamp()),
            TimeUnit::Millisecond => Ok(timestamp.timestamp_millis()),
            TimeUnit::Microsecond => Ok(timestamp.timestamp_micros()),
            TimeUnit::Nanosecond => {
                timestamp.timestamp_nanos_opt().ok_or_else(|| {
                    Error::out_of_range("timestamp is outside Arrow range")
                })
            }
        }
    }

    /// Returns the day of the timestamp.
    pub fn day(&self) -> u8 {
        self.day
    }

    /// Returns the hour of the timestamp.
    pub fn hour(&self) -> u8 {
        self.hour
    }

    /// Returns whether the timestamp is a UTC timestamp or not. A timestamp
    /// with a time zone is a UTC timestamp: its date and time are in UTC and
    /// [`tz_hour_offset()`](Self::tz_hour_offset) and
    /// [`tz_minute_offset()`](Self::tz_minute_offset) return the offset of
    /// its time zone.
    pub fn is_utc(&self) -> bool {
        self.is_utc
    }

    /// Returns the minute of the timestamp.
    pub fn minute(&self) -> u8 {
        self.minute
    }

    /// Returns the month of the timestamp.
    pub fn month(&self) -> u8 {
        self.month
    }

    /// Returns the nanoseconds of the timestamp.
    pub fn nanoseconds(&self) -> u32 {
        self.nanoseconds
    }

    /// Creates a new timestamp given only the date. The time portion is set
    /// to be midnight.
    pub fn new_date(year: i16, month: u8, day: u8) -> Self {
        Self {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            nanoseconds: 0,
            is_utc: false,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        }
    }

    /// Creates a new naive timestamp given the date and time.
    pub fn new_timestamp(
        year: i16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanoseconds: u32,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanoseconds,
            is_utc: false,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        }
    }

    /// Creates a new UTC timestamp given the date and time.
    pub fn new_utc_timestamp(
        year: i16,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanoseconds: u32,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            nanoseconds,
            is_utc: true,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        }
    }

    /// Returns the second of the timestamp.
    pub fn second(&self) -> u8 {
        self.second
    }

    /// Returns the hour portion of the time zone offset of the timestamp. It
    /// is negative for time zones west of UTC and zero for timestamps without
    /// a time zone.
    pub fn tz_hour_offset(&self) -> i8 {
        self.tz_hour_offset
    }

    /// Returns the minute portion of the time zone offset of the timestamp. It
    /// has the same sign as the whole offset, so -03:30 has an hour portion of
    /// -3 and a minute portion of -30.
    pub fn tz_minute_offset(&self) -> i8 {
        self.tz_minute_offset
    }

    /// Returns the same instant as a timestamp with a time zone that has the
    /// given offset from UTC. The date and time of the timestamp are taken as
    /// UTC and are not changed; the offset is the one stored in a TIMESTAMP
    /// WITH TIME ZONE value. Both portions of the offset have the same sign,
    /// so -03:30 is given as -3 hours and -30 minutes. An error is returned if
    /// the hours are not between -15 and 15, the minutes are not between -59
    /// and 59 or the two portions have different signs.
    pub fn with_tz_offset(
        mut self,
        hours: i8,
        minutes: i8,
    ) -> Result<Self, Error> {
        if !(-15..=15).contains(&hours)
            || !(-59..=59).contains(&minutes)
            || (hours > 0 && minutes < 0)
            || (hours < 0 && minutes > 0)
        {
            return Err(Error::out_of_range(format!(
                "invalid time zone offset: {hours} hours and {minutes} minutes"
            )));
        }
        self.is_utc = true;
        self.tz_hour_offset = hours;
        self.tz_minute_offset = minutes;
        Ok(self)
    }

    /// Returns the year of the timestamp.
    pub fn year(&self) -> i16 {
        self.year
    }
}

impl fmt::Display for OracleTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        )?;
        if self.nanoseconds != 0 {
            write!(f, ".{:09}", self.nanoseconds)?;
        }
        if self.is_utc {
            f.write_char('Z')?;
        }
        Ok(())
    }
}

impl fmt::Debug for OracleTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut tuple = f.debug_tuple("OracleTimestamp");
        tuple.field(&self.to_string());
        if self.tz_hour_offset != 0 || self.tz_minute_offset != 0 {
            let sign = if self.tz_hour_offset < 0 || self.tz_minute_offset < 0
            {
                '-'
            } else {
                '+'
            };
            tuple.field(&format!(
                "{}{:02}:{:02}",
                sign,
                self.tz_hour_offset.unsigned_abs(),
                self.tz_minute_offset.unsigned_abs()
            ));
        }
        tuple.finish()
    }
}

impl FromBuf for OracleTimestamp {
    fn from_buf(buf: &[u8]) -> Self {
        let mut nanoseconds: u32 = 0;
        if buf.len() >= constants::ORA_TYPE_SIZE_TIMESTAMP {
            nanoseconds = u32::from_be_bytes(buf[7..11].try_into().unwrap());
        }
        let is_utc = buf.len() == constants::ORA_TYPE_SIZE_TIMESTAMP_TZ;
        // the date and time are always in UTC; the last two bytes hold either
        // the offset of the time zone or the id of a time zone region, whose
        // offset is not known on the client
        let mut tz_hour_offset = 0;
        let mut tz_minute_offset = 0;
        if is_utc
            && buf[11] & constants::TZ_HAS_REGION_ID == 0
            && buf[11] != 0
            && buf[12] != 0
        {
            tz_hour_offset = buf[11] as i8 - constants::TZ_HOUR_OFFSET as i8;
            tz_minute_offset =
                buf[12] as i8 - constants::TZ_MINUTE_OFFSET as i8;
        }
        Self {
            year: ((buf[0] as i16 - 100) * 100 + buf[1] as i16 - 100),
            month: buf[2],
            day: buf[3],
            hour: buf[4] - 1,
            minute: buf[5] - 1,
            second: buf[6] - 1,
            nanoseconds,
            is_utc,
            tz_hour_offset,
            tz_minute_offset,
        }
    }
}

impl ToBuf for OracleTimestamp {
    fn to_buf(
        &self,
        buf: &mut WriteBuffer,
        db_type: &'static DbType,
        write_length: bool,
    ) {
        let mut actual_db_type = db_type;
        if db_type == crate::DB_TYPE_TIMESTAMP && self.nanoseconds() == 0 {
            actual_db_type = crate::DB_TYPE_DATE;
        }
        if write_length {
            buf.write_u8(
                actual_db_type.buffer_size_factor.try_into().unwrap(),
            );
        }
        buf.write_u8((self.year() / 100 + 100).try_into().unwrap());
        buf.write_u8((self.year() % 100 + 100).try_into().unwrap());
        buf.write_u8(self.month());
        buf.write_u8(self.day());
        buf.write_u8(self.hour() + 1);
        buf.write_u8(self.minute() + 1);
        buf.write_u8(self.second() + 1);
        if actual_db_type != crate::DB_TYPE_DATE {
            buf.write_u32be(self.nanoseconds());
            if actual_db_type == crate::DB_TYPE_TIMESTAMP_TZ {
                buf.write_u8(
                    (constants::TZ_HOUR_OFFSET as i8 + self.tz_hour_offset)
                        as u8,
                );
                buf.write_u8(
                    (constants::TZ_MINUTE_OFFSET as i8 + self.tz_minute_offset)
                        as u8,
                );
            }
        }
    }
}

impl From<chrono::NaiveDateTime> for OracleTimestamp {
    fn from(value: chrono::NaiveDateTime) -> Self {
        Self {
            year: value.year() as i16,
            month: value.month() as u8,
            day: value.day() as u8,
            hour: value.hour() as u8,
            minute: value.minute() as u8,
            second: value.second() as u8,
            nanoseconds: value.nanosecond(),
            is_utc: false,
            tz_hour_offset: 0,
            tz_minute_offset: 0,
        }
    }
}

impl TryFrom<OracleTimestamp> for chrono::NaiveDateTime {
    type Error = Error;
    fn try_from(ts: OracleTimestamp) -> Result<Self, Self::Error> {
        chrono::NaiveDate::from_ymd_opt(
            ts.year().into(),
            ts.month().into(),
            ts.day().into(),
        )
        .ok_or_else(|| {
            Error::out_of_range(format!(
                "invalid year-month-day: {}-{}-{}",
                ts.year(),
                ts.month(),
                ts.day()
            ))
        })?
        .and_hms_nano_opt(
            ts.hour().into(),
            ts.minute().into(),
            ts.second().into(),
            ts.nanoseconds(),
        )
        .ok_or_else(|| {
            Error::out_of_range(format!(
                "invalid timestamp: {}-{}-{} {}:{}:{}.{:09}",
                ts.year(),
                ts.month(),
                ts.day(),
                ts.hour(),
                ts.minute(),
                ts.second(),
                ts.nanoseconds()
            ))
        })
    }
}
