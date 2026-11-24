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
// object.rs
//
// Retains database object images and decodes them when taken from a row.
// Builds objects and collections on the client and encodes them as images
// for binds.
//-----------------------------------------------------------------------------

use std::sync::Arc;

use super::DbObjectType;
use crate::client::Client;
use crate::client::ClientRef;
use crate::column_index::ColumnIndex;
use crate::constants;
use crate::db_type::DbType;
use crate::db_value::{DbValue, FromDbValue, ToDbValue};
use crate::error::Error;
use crate::lob::{Lob, PendingLobData};
use crate::metadata::Metadata;
use crate::read_buffer::FromBuf;
use crate::read_buffer::ReadBuffer;
use crate::response::Response;
use crate::row::{ColumnData, DbRow, Row};
use crate::write_buffer::{ToBuf, WriteBuffer};

const TTC_OBJ_ATOMIC_NULL: u8 = 253;
const TTC_OBJ_IS_DEGENERATE: u8 = 0x10;
const TTC_OBJ_NO_PREFIX_SEG: u8 = 0x04;
pub(super) const TTC_OBJ_PLSQL_INDEX_TABLE: u8 = 1;
const TTC_OBJ_IS_VERSION_81: u8 = 0x80;
const TTC_OBJ_IS_COLLECTION: u8 = 0x08;
const TTC_OBJ_IMAGE_VERSION: u8 = 1;
const TTC_OBJ_MAX_SHORT_LENGTH: usize = 245;
const TTC_OBJ_TOP_LEVEL: u32 = 0x01;
const TTC_OBJ_HAS_INDEXES: u8 = 0x10;
const TTC_OBJ_NON_NULL_OID: u8 = 0x02;
const TTC_OBJ_HAS_EXTENT_OID: u8 = 0x08;
const TTC_OBJ_EXTENT_OID: [u8; 16] =
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1];

#[derive(Debug)]
/// A fetched database collection.
pub struct DbCollection {
    object_type: Arc<DbObjectType>,
    values: DbRow,
    indices: Option<Vec<i32>>,
}

/// A fetched database object.
#[derive(Debug)]
pub struct DbObject {
    object_type: Arc<DbObjectType>,
    data: Row,
}

/// A simple enumeration to simplify decoding of images which may contain a
/// database object or a collection.
enum DbObjectOrCollection {
    DbCollection(DbCollection),
    DbObject(DbObject),
}

/// Holds an object or collection image until its type has been resolved.
pub(crate) struct PendingDbObject {
    bytes: Vec<u8>,
}

impl DbCollection {
    /// Creates a database collection from decoded values.
    fn from_values(
        object_type: &Arc<DbObjectType>,
        values: DbRow,
        indices: Option<Vec<i32>>,
    ) -> Self {
        Self { object_type: Arc::clone(object_type), values, indices }
    }

    /// Creates an empty collection of the given type, ready to be filled with
    /// [`push`](Self::push) (nested tables and VARRAYs) or
    /// [`insert`](Self::insert) (associative arrays) and bound.
    ///
    /// Fails with [`ErrorKind::WrongDbObjectTypeKind`](crate::ErrorKind)
    /// when the type is not a collection type.
    pub fn new(object_type: &Arc<DbObjectType>) -> Result<Self, Error> {
        if !object_type.is_collection() {
            return Err(Error::wrong_db_object_type_kind(
                object_type.fqn(),
                "a collection type: use DbObject::new()",
            ));
        }
        let indices = object_type.is_associative_array().then(Vec::new);
        Ok(Self::from_values(object_type, DbRow::new(Vec::new()), indices))
    }

    /// Appends an element to a nested table or VARRAY. The value is checked
    /// against the element type (see [`DbObject::set`]). A VARRAY's limit is
    /// checked by the database when the collection is bound (ORA-22165).
    ///
    /// Fails with [`ErrorKind::WrongDbObjectTypeKind`](crate::ErrorKind)
    /// for an associative array, which takes [`insert`](Self::insert).
    pub fn push(
        &mut self,
        value: impl IntoDbObjectValue,
    ) -> Result<(), Error> {
        if self.indices.is_some() {
            return Err(Error::wrong_db_object_type_kind(
                self.object_type.fqn(),
                "a nested table or VARRAY: use insert()",
            ));
        }
        let metadata = self.object_type.element_metadata().unwrap();
        let value = coerce(metadata, value.into_db_object_value()?)?;
        self.values.values_mut().push(value);
        Ok(())
    }

    /// Sets the element of an associative array at the given key, replacing
    /// any element already there. Keys are kept in ascending order, as PL/SQL
    /// does. The value is checked against the element type (see
    /// [`DbObject::set`]).
    ///
    /// Fails with [`ErrorKind::WrongDbObjectTypeKind`](crate::ErrorKind)
    /// for a nested table or VARRAY, which take [`push`](Self::push).
    pub fn insert(
        &mut self,
        key: i32,
        value: impl IntoDbObjectValue,
    ) -> Result<(), Error> {
        let Some(indices) = self.indices.as_mut() else {
            return Err(Error::wrong_db_object_type_kind(
                self.object_type.fqn(),
                "an associative array: use push()",
            ));
        };
        let metadata = self.object_type.element_metadata().unwrap();
        let value = coerce(metadata, value.into_db_object_value()?)?;
        let values = self.values.values_mut();
        match indices.binary_search(&key) {
            Ok(pos) => values[pos] = value,
            Err(pos) => {
                indices.insert(pos, key);
                values.insert(pos, value);
            }
        }
        Ok(())
    }

    /// Returns the keys of an associative array in ascending order, or None
    /// for a nested table or VARRAY.
    pub fn keys(&self) -> Option<&[i32]> {
        self.indices.as_deref()
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.values.values().len()
    }

    /// Reports whether the collection has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the packed image of the collection.
    fn packed_image(&self) -> Vec<u8> {
        let mut writer = ImageWriter::new(&self.object_type);
        writer.write_collection_body(self);
        writer.finish()
    }

    /// Transforms an owned DbCollection into a vector of the requested type.
    fn transform_owned<'a, T>(self) -> Result<Vec<T>, Error>
    where
        T: FromDbValue<'a>,
    {
        self.values.transform_owned()
    }

    /// Returns the type describing this collection.
    pub fn object_type(&self) -> &Arc<DbObjectType> {
        &self.object_type
    }
}

impl DbObject {
    /// Creates a database object from decoded values.
    fn from_row(object_type: &Arc<DbObjectType>, data: Row) -> Self {
        Self { object_type: Arc::clone(object_type), data }
    }

    /// Creates an object of the given type with every attribute NULL, ready to
    /// be filled with [`set`](Self::set) and bound.
    ///
    /// Fails with [`ErrorKind::WrongDbObjectTypeKind`](crate::ErrorKind)
    /// for a collection type, and with
    /// [`ErrorKind::UnsupportedDbObjectSubtype`](crate::ErrorKind) for a type
    /// declared UNDER another type: instances of subtypes are not supported.
    pub fn new(object_type: &Arc<DbObjectType>) -> Result<Self, Error> {
        if object_type.is_collection() {
            return Err(Error::wrong_db_object_type_kind(
                object_type.fqn(),
                "an object type: use DbCollection::new()",
            ));
        }
        if object_type.is_subtype() {
            return Err(Error::unsupported_db_object_subtype(
                object_type.fqn(),
            ));
        }
        let attrs = object_type.attributes();
        let row = Row::new(
            &Arc::new(attrs.to_vec()),
            DbRow::new(attrs.iter().map(|_| None).collect()),
        );
        Ok(Self::from_row(object_type, row))
    }

    /// Sets an object attribute by name or zero-based position. The value is
    /// checked against the attribute's type and converted to it:
    ///
    /// - NUMBER and BINARY_INTEGER take integers, [`OracleNumber`](crate::OracleNumber),
    ///   `f32` and `f64`; BINARY_FLOAT and BINARY_DOUBLE take the same;
    /// - CHAR, NCHAR, VARCHAR2 and NVARCHAR2 take strings, RAW takes bytes,
    ///   each within the attribute's maximum size in bytes (national strings
    ///   count as UTF-16); CHAR and NCHAR values are stored as given, without
    ///   blank-padding;
    /// - BOOLEAN takes `bool`;
    /// - DATE, TIMESTAMP and the TIMESTAMP WITH (LOCAL) TIME ZONE types take
    ///   [`OracleTimestamp`](crate::OracleTimestamp);
    /// - INTERVAL DAY TO SECOND and INTERVAL YEAR TO MONTH take
    ///   [`OracleIntervalDS`](crate::OracleIntervalDS) and
    ///   [`OracleIntervalYM`](crate::OracleIntervalYM);
    /// - an object or collection attribute takes a [`DbObject`] or
    ///   [`DbCollection`] of exactly its type.
    ///
    /// `None` stores NULL in any attribute. A value that does not fit fails
    /// with [`ErrorKind::CannotStoreDbObjectValue`](crate::ErrorKind). A
    /// value for an attribute of another type (LOB, BFILE, ...) fails with
    /// [`ErrorKind::UnsupportedDbObjectAttributeType`](crate::ErrorKind).
    pub fn set(
        &mut self,
        index: impl ColumnIndex,
        value: impl IntoDbObjectValue,
    ) -> Result<(), Error> {
        let attrs = self.object_type.attributes();
        let index = index.resolve(attrs)?;
        let metadata = attrs
            .get(index)
            .ok_or_else(|| Error::invalid_column_index(index))?;
        let value = coerce(metadata, value.into_db_object_value()?)?;
        self.data.db_row_mut().values_mut()[index] = value;
        Ok(())
    }

    /// Returns the packed image of the object.
    fn packed_image(&self) -> Vec<u8> {
        let mut writer = ImageWriter::new(&self.object_type);
        writer.write_object_body(self);
        writer.finish()
    }

    /// Returns an object attribute by name or zero-based position.
    pub fn get<'a, T: FromDbValue<'a>>(
        &'a self,
        index: impl ColumnIndex,
    ) -> Result<T, Error> {
        self.data.get(index)
    }

    /// Returns the type describing this value.
    pub fn object_type(&self) -> &Arc<DbObjectType> {
        &self.object_type
    }

    /// Transfers an object attribute to the caller, replacing it with NULL.
    pub fn take<'a, T: FromDbValue<'a>>(
        &'a mut self,
        index: impl ColumnIndex,
    ) -> Result<T, Error> {
        self.data.take(index)
    }
}

impl PendingDbObject {
    /// Reads the TTC envelope and retains the packed image.
    pub(crate) fn from_response(
        resp: &mut Response,
    ) -> Result<Option<Self>, Error> {
        let _type_oid = resp.read_bytes_with_double_length()?;
        let _instance_oid = resp.read_bytes_with_double_length()?;
        let _snapshot = resp.read_bytes_with_double_length()?;
        let _version = resp.read_ub2()?;
        let size = resp.read_ub4()?;
        let _flags = resp.read_ub2()?;
        if size == 0 {
            Ok(None)
        } else {
            Ok(Some(Self {
                bytes: resp.read_bytes_with_length()?.into_owned(),
            }))
        }
    }

    /// Attempt to decode a database object and return it upon success.
    pub(crate) fn try_into_db_value(
        self,
        object_type: &Arc<DbObjectType>,
        client: &Client,
        client_ref: &ClientRef,
    ) -> Result<DbValue, Error> {
        let mut buf = DbObjectReadBuffer::new(&self.bytes, client, client_ref);
        match buf.decode_image(object_type)? {
            DbObjectOrCollection::DbCollection(coll) => {
                Ok(DbValue::DbCollection(coll))
            }
            DbObjectOrCollection::DbObject(obj) => Ok(DbValue::DbObject(obj)),
        }
    }
}

impl<T> TryFrom<DbCollection> for Vec<T>
where
    for<'a> T: FromDbValue<'a>,
{
    type Error = Error;

    fn try_from(source: DbCollection) -> Result<Self, Self::Error> {
        source.transform_owned()
    }
}

impl<'a> FromDbValue<'a> for DbObject {
    fn from_db_value(data: ColumnData<'a>) -> Result<Self, Error> {
        match data {
            ColumnData::Owned(Some(DbValue::DbObject(obj))) => Ok(obj),
            _ => Err(Self::unsupported_conversion(data)),
        }
    }
}

impl<'a> FromDbValue<'a> for DbCollection {
    fn from_db_value(data: ColumnData<'a>) -> Result<Self, Error> {
        match data {
            ColumnData::Owned(Some(DbValue::DbCollection(coll))) => Ok(coll),
            _ => Err(Self::unsupported_conversion(data)),
        }
    }
}

struct DbObjectReadBuffer<'a> {
    buf: ReadBuffer,
    client: &'a Client,
    client_ref: &'a ClientRef,
}

impl DbObjectReadBuffer<'_> {
    /// Decodes the body of a database object or collection and returns a
    /// vector of database values adn a vector of indices (for collections).
    fn decode_body(
        &mut self,
        object_type: &Arc<DbObjectType>,
    ) -> Result<DbObjectOrCollection, Error> {
        let mut values = Vec::new();
        if object_type.is_collection() {
            let collection_type = object_type.collection_type();
            let metadata = object_type.element_metadata().unwrap();
            let _collection_flags = self.buf.read_u8()?;
            let num_elements = self.read_length()?;
            let mut indices = (collection_type
                == Some(TTC_OBJ_PLSQL_INDEX_TABLE))
            .then(|| Vec::with_capacity(num_elements));
            for _ in 0..num_elements {
                if let Some(indices) = indices.as_mut() {
                    indices.push(self.buf.read_u32be()? as i32);
                }
                values.push(self.decode_value(object_type, metadata)?);
            }
            Ok(DbObjectOrCollection::DbCollection(DbCollection::from_values(
                object_type,
                DbRow::new(values),
                indices,
            )))
        } else {
            for metadata in object_type.attributes() {
                values.push(self.decode_value(object_type, metadata)?);
            }
            let row = Row::new(
                &Arc::new(object_type.attributes().to_vec()),
                DbRow::new(values),
            );
            Ok(DbObjectOrCollection::DbObject(DbObject::from_row(
                object_type,
                row,
            )))
        }
    }

    /// Decodes a database object or collection image and returns a vector of
    /// database values and a vector of indices (for collections).
    fn decode_image(
        &mut self,
        object_type: &Arc<DbObjectType>,
    ) -> Result<DbObjectOrCollection, Error> {
        let flags = self.buf.read_u8()?;
        let _image_version = self.buf.read_u8()?;
        let _image_size = self.read_length()?;
        if flags & TTC_OBJ_IS_DEGENERATE != 0 {
            return Err(Error::not_implemented(
                "database object stored in a LOB".into(),
            ));
        }
        if flags & TTC_OBJ_NO_PREFIX_SEG == 0 {
            // an object (not a collection) has a prefix segment only when it
            // is an instance of a subtype: the segment names that subtype,
            // whose attributes follow the declared type's ones
            if !object_type.is_collection() {
                return Err(Error::unsupported_db_object_subtype(
                    object_type.fqn(),
                ));
            }
            let length = self.read_length()?;
            let _prefix_seg = self.buf.read_bytes(length)?;
        }
        self.decode_body(object_type)
    }

    /// Decodes a single value from the image.
    fn decode_value(
        &mut self,
        object_type: &Arc<DbObjectType>,
        metadata: &Metadata,
    ) -> Result<Option<DbValue>, Error> {
        let client = self.client;
        let client_ref = self.client_ref;
        let marker = self.peek_u8()?;
        let typ = metadata.db_type();
        if typ == crate::DB_TYPE_OBJECT {
            let child_object_type = metadata
                .object_type()?
                .ok_or_else(Error::db_object_type_unavailable)?;
            let has_image = has_own_image(object_type, &child_object_type);
            if marker == TTC_OBJ_ATOMIC_NULL
                || (has_image
                    && marker == constants::TTC_NULL_LENGTH_INDICATOR)
            {
                self.buf.read_u8()?;
                return Ok(None);
            }
            let result = if has_image {
                let mut child_buf = DbObjectReadBuffer::new(
                    self.read_bytes_with_length()?,
                    client,
                    client_ref,
                );
                child_buf.decode_image(&child_object_type)?
            } else {
                self.decode_body(&child_object_type)?
            };
            return match result {
                DbObjectOrCollection::DbCollection(coll) => {
                    Ok(Some(DbValue::DbCollection(coll)))
                }
                DbObjectOrCollection::DbObject(obj) => {
                    Ok(Some(DbValue::DbObject(obj)))
                }
            };
        }
        if marker == 0 || marker == constants::TTC_NULL_LENGTH_INDICATOR {
            self.buf.read_u8()?;
            return Ok(None);
        }
        let bytes = self.read_bytes_with_length()?;
        validate_scalar_length(typ, bytes.len())?;
        let scalar = match typ {
            crate::DB_TYPE_CHAR
            | crate::DB_TYPE_VARCHAR
            | crate::DB_TYPE_NCHAR
            | crate::DB_TYPE_NVARCHAR => {
                let charset = {
                    if typ.csfrm == constants::CS_FORM_NCHAR {
                        client.get_ncharset_id()
                    } else {
                        client.get_charset_id()
                    }
                };
                let value = match charset {
                    crate::constants::CHARSET_ID_UTF8 => {
                        String::from_utf8(bytes.to_vec())?
                    }
                    crate::constants::CHARSET_ID_UTF16 => {
                        crate::utils::utf16be_bytes_to_string(bytes)?
                    }
                    _ => {
                        return Err(Error::not_implemented(format!(
                            "object character set {charset}"
                        )));
                    }
                };
                DbValue::String(value)
            }
            crate::DB_TYPE_RAW => DbValue::Bytes(bytes.to_vec()),
            crate::DB_TYPE_NUMBER => {
                DbValue::Number(crate::OracleNumber::from_buf(bytes))
            }
            crate::DB_TYPE_BINARY_INTEGER => {
                let value =
                    i32::from_be_bytes(bytes.to_vec().try_into().unwrap());
                DbValue::Number(value.to_string().parse()?)
            }
            crate::DB_TYPE_BINARY_FLOAT => {
                DbValue::BinaryFloat(f32::from_buf(bytes))
            }
            crate::DB_TYPE_BINARY_DOUBLE => {
                DbValue::BinaryDouble(f64::from_buf(bytes))
            }
            crate::DB_TYPE_BOOLEAN => DbValue::Boolean(bool::from_buf(bytes)),
            crate::DB_TYPE_INTERVAL_DS => {
                DbValue::IntervalDS(crate::OracleIntervalDS::from_buf(bytes))
            }
            crate::DB_TYPE_INTERVAL_YM => {
                DbValue::IntervalYM(crate::OracleIntervalYM::from_buf(bytes))
            }
            value if value.is_date_type() => {
                if bytes.len() == 13 && bytes[11] & 0x80 != 0 {
                    return Err(Error::not_implemented(
                        "object timestamp with a time zone region".into(),
                    ));
                }
                DbValue::Timestamp(crate::OracleTimestamp::from_buf(bytes))
            }
            crate::DB_TYPE_CLOB
            | crate::DB_TYPE_NCLOB
            | crate::DB_TYPE_BLOB
            | crate::DB_TYPE_BFILE => DbValue::Lob(Lob::new(
                client_ref.clone(),
                PendingLobData::from_locator(typ, bytes.to_vec()),
            )),
            _ => return Err(Error::unsupported_db_type(typ)),
        };
        Ok(Some(scalar))
    }

    /// Creates a new buffer for reading database object and collections.
    fn new<'a>(
        data: &[u8],
        client: &'a Client,
        client_ref: &'a ClientRef,
    ) -> DbObjectReadBuffer<'a> {
        DbObjectReadBuffer {
            buf: ReadBuffer::from_bytes(data),
            client,
            client_ref,
        }
    }

    /// Peeks at the next byte without advancing the reader.
    fn peek_u8(&mut self) -> Result<u8, Error> {
        let pos = self.buf.get_pos();
        let value = self.buf.read_u8()?;
        self.buf.set_pos(pos);
        Ok(value)
    }

    /// Reads bytes prefixed by an object-image length.
    fn read_bytes_with_length(&mut self) -> Result<&[u8], Error> {
        let length = self.read_length()?;
        self.buf.read_bytes(length)
    }

    /// Reads a short or extended object-image length.
    fn read_length(&mut self) -> Result<usize, Error> {
        let length = self.buf.read_u8()?;
        if length == crate::constants::TTC_LONG_LENGTH_INDICATOR {
            Ok(self.buf.read_u32be()? as usize)
        } else {
            Ok(length as usize)
        }
    }
}

/// Validates the encoded length of a scalar object value.
fn validate_scalar_length(
    typ: &'static crate::DbType,
    length: usize,
) -> Result<(), Error> {
    use crate::constants;

    let expected = match typ {
        crate::DB_TYPE_BINARY_FLOAT => {
            Some(constants::ORA_TYPE_SIZE_BINARY_FLOAT)
        }
        crate::DB_TYPE_BINARY_INTEGER => {
            Some(constants::ORA_TYPE_SIZE_BINARY_INTEGER)
        }
        crate::DB_TYPE_BOOLEAN => Some(constants::ORA_TYPE_SIZE_BOOLEAN),
        crate::DB_TYPE_BINARY_DOUBLE => {
            Some(constants::ORA_TYPE_SIZE_BINARY_DOUBLE)
        }
        crate::DB_TYPE_INTERVAL_DS => {
            Some(constants::ORA_TYPE_SIZE_INTERVAL_DS)
        }
        crate::DB_TYPE_INTERVAL_YM => {
            Some(constants::ORA_TYPE_SIZE_INTERVAL_YM)
        }
        _ => None,
    };
    let valid_date_length = matches!(
        length,
        constants::ORA_TYPE_SIZE_DATE
            | constants::ORA_TYPE_SIZE_TIMESTAMP
            | constants::ORA_TYPE_SIZE_TIMESTAMP_TZ
    );
    if expected.is_some_and(|size| length != size)
        || (typ == crate::DB_TYPE_NUMBER && length == 0)
        || (typ.is_date_type() && !valid_date_length)
    {
        return Err(Error::unexpected_result());
    }
    Ok(())
}

/// Reports whether an attribute or element of `child` type, inside a value of
/// `parent` type, is written as an image of its own (prefixed by its length)
/// rather than inline. Elements of collections, collections and objects of a
/// NOT FINAL (substitutable) type have their own image.
fn has_own_image(parent: &DbObjectType, child: &DbObjectType) -> bool {
    parent.is_collection() || child.is_collection() || !child.is_final()
}

/// A Rust value that can be stored in an attribute of a [`DbObject`] or an
/// element of a [`DbCollection`] built on the client. `None` stores NULL.
pub trait IntoDbObjectValue {
    #[doc(hidden)]
    fn into_db_object_value(self) -> Result<Option<DbValue>, Error>;
}

macro_rules! into_db_object_value_via_number {
    ($($t:ty),*) => {
        $(
            impl IntoDbObjectValue for $t {
                fn into_db_object_value(
                    self,
                ) -> Result<Option<DbValue>, Error> {
                    Ok(Some(DbValue::Number(self.to_string().parse()?)))
                }
            }
        )*
    };
}

into_db_object_value_via_number!(
    i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
);

macro_rules! into_db_object_value_direct {
    ($($t:ty => $variant:ident($conv:expr)),* $(,)?) => {
        $(
            impl IntoDbObjectValue for $t {
                fn into_db_object_value(
                    self,
                ) -> Result<Option<DbValue>, Error> {
                    Ok(Some(DbValue::$variant($conv(self))))
                }
            }
        )*
    };
}

into_db_object_value_direct!(
    crate::OracleNumber => Number(|v| v),
    f64 => BinaryDouble(|v| v),
    f32 => BinaryFloat(|v| v),
    bool => Boolean(|v| v),
    String => String(|v| v),
    &str => String(|v: &str| v.to_string()),
    Vec<u8> => Bytes(|v| v),
    &[u8] => Bytes(|v: &[u8]| v.to_vec()),
    crate::OracleTimestamp => Timestamp(|v| v),
    crate::OracleIntervalDS => IntervalDS(|v| v),
    crate::OracleIntervalYM => IntervalYM(|v| v),
    DbObject => DbObject(|v| v),
    DbCollection => DbCollection(|v| v),
);

impl<T: IntoDbObjectValue> IntoDbObjectValue for Option<T> {
    fn into_db_object_value(self) -> Result<Option<DbValue>, Error> {
        match self {
            Some(value) => value.into_db_object_value(),
            None => Ok(None),
        }
    }
}

/// Reports whether values of the given attribute or element type can be
/// written in an object image built on the client.
fn is_writable_type(typ: &'static DbType) -> bool {
    matches!(
        typ,
        crate::DB_TYPE_CHAR
            | crate::DB_TYPE_VARCHAR
            | crate::DB_TYPE_NCHAR
            | crate::DB_TYPE_NVARCHAR
            | crate::DB_TYPE_RAW
            | crate::DB_TYPE_NUMBER
            | crate::DB_TYPE_BINARY_INTEGER
            | crate::DB_TYPE_BINARY_FLOAT
            | crate::DB_TYPE_BINARY_DOUBLE
            | crate::DB_TYPE_BOOLEAN
            | crate::DB_TYPE_INTERVAL_DS
            | crate::DB_TYPE_INTERVAL_YM
            | crate::DB_TYPE_OBJECT
    ) || typ.is_date_type()
}

/// Checks a value against the attribute or element metadata and converts it
/// to the representation the image encoder writes for that type.
fn coerce(
    metadata: &Metadata,
    value: Option<DbValue>,
) -> Result<Option<DbValue>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let typ = metadata.db_type();
    let target = if metadata.name().is_empty() {
        "a collection element".to_string()
    } else {
        format!("attribute {}", metadata.name())
    };
    if !is_writable_type(typ) {
        return Err(Error::unsupported_db_object_attribute_type(
            target,
            metadata.data_type(),
        ));
    }
    let cannot_store = |what: String| {
        Error::cannot_store_db_object_value(
            what,
            target.as_str(),
            metadata.data_type(),
        )
    };
    let mismatch = |value: &DbValue| {
        cannot_store(format!("a {} value", value.type_name()))
    };
    // the server does not check the length of an attribute or element in an
    // image, so a value that is too long would be stored as is; the maximum
    // size is in bytes, as encoded in the image
    let check_length = |length: usize| {
        let max = metadata.max_size() as usize;
        if max > 0 && length > max {
            return Err(cannot_store(format!(
                "a value of {length} bytes (the maximum is {max})"
            )));
        }
        Ok(())
    };
    let to_number = |text: String| {
        text.parse::<crate::OracleNumber>()
            .map_err(|_| cannot_store(format!("the value {text}")))
    };
    let to_f64 = |text: String| {
        text.parse::<f64>()
            .map_err(|_| cannot_store(format!("the value {text}")))
    };
    let value = match (typ, value) {
        (crate::DB_TYPE_NUMBER, DbValue::Number(v)) => DbValue::Number(v),
        (crate::DB_TYPE_NUMBER, DbValue::BinaryDouble(v)) => {
            DbValue::Number(to_number(v.to_string())?)
        }
        (crate::DB_TYPE_NUMBER, DbValue::BinaryFloat(v)) => {
            DbValue::Number(to_number(v.to_string())?)
        }
        (crate::DB_TYPE_BINARY_INTEGER, DbValue::Number(v)) => {
            if v.to_string().parse::<i32>().is_err() {
                return Err(cannot_store(format!("the value {v}")));
            }
            DbValue::Number(v)
        }
        (crate::DB_TYPE_BINARY_DOUBLE, DbValue::BinaryDouble(v)) => {
            DbValue::BinaryDouble(v)
        }
        (crate::DB_TYPE_BINARY_DOUBLE, DbValue::BinaryFloat(v)) => {
            DbValue::BinaryDouble(v.into())
        }
        (crate::DB_TYPE_BINARY_DOUBLE, DbValue::Number(v)) => {
            DbValue::BinaryDouble(to_f64(v.to_string())?)
        }
        (crate::DB_TYPE_BINARY_FLOAT, DbValue::BinaryFloat(v)) => {
            DbValue::BinaryFloat(v)
        }
        (crate::DB_TYPE_BINARY_FLOAT, DbValue::BinaryDouble(v)) => {
            DbValue::BinaryFloat(v as f32)
        }
        (crate::DB_TYPE_BINARY_FLOAT, DbValue::Number(v)) => {
            DbValue::BinaryFloat(to_f64(v.to_string())? as f32)
        }
        (
            crate::DB_TYPE_CHAR
            | crate::DB_TYPE_VARCHAR
            | crate::DB_TYPE_NCHAR
            | crate::DB_TYPE_NVARCHAR,
            DbValue::String(v),
        ) => {
            // national strings are written in UTF-16
            check_length(if typ.csfrm == constants::CS_FORM_NCHAR {
                v.encode_utf16().count() * 2
            } else {
                v.len()
            })?;
            DbValue::String(v)
        }
        (crate::DB_TYPE_RAW, DbValue::Bytes(v)) => {
            check_length(v.len())?;
            DbValue::Bytes(v)
        }
        (crate::DB_TYPE_BOOLEAN, DbValue::Boolean(v)) => DbValue::Boolean(v),
        (t, DbValue::Timestamp(v)) if t.is_date_type() => {
            DbValue::Timestamp(v)
        }
        (crate::DB_TYPE_INTERVAL_DS, DbValue::IntervalDS(v)) => {
            DbValue::IntervalDS(v)
        }
        (crate::DB_TYPE_INTERVAL_YM, DbValue::IntervalYM(v)) => {
            DbValue::IntervalYM(v)
        }
        (crate::DB_TYPE_OBJECT, value) => {
            let expected = metadata
                .object_type()?
                .ok_or_else(Error::db_object_type_unavailable)?;
            let actual = match &value {
                DbValue::DbObject(v) => v.object_type(),
                DbValue::DbCollection(v) => v.object_type(),
                _ => return Err(mismatch(&value)),
            };
            if actual.fqn() != expected.fqn() {
                return Err(cannot_store(format!("a {}", actual.fqn())));
            }
            value
        }
        (_, value) => return Err(mismatch(&value)),
    };
    Ok(Some(value))
}

/// Encodes an object or collection image: the inverse of
/// `DbObjectReadBuffer`.
struct ImageWriter {
    buf: Vec<u8>,
}

impl ImageWriter {
    /// Starts an image for the given type. The image size is written by
    /// `finish()` once it is known.
    fn new(object_type: &DbObjectType) -> Self {
        let mut flags = TTC_OBJ_IS_VERSION_81;
        if object_type.is_collection() {
            flags |= TTC_OBJ_IS_COLLECTION;
        } else {
            flags |= TTC_OBJ_NO_PREFIX_SEG;
        }
        let mut buf = vec![
            flags,
            TTC_OBJ_IMAGE_VERSION,
            constants::TTC_LONG_LENGTH_INDICATOR,
            0,
            0,
            0,
            0,
        ];
        if object_type.is_collection() {
            buf.push(1); // length of the prefix segment
            buf.push(1); // prefix segment contents
        }
        Self { buf }
    }

    /// Writes the image size into the header and returns the image.
    fn finish(mut self) -> Vec<u8> {
        let size = self.buf.len() as u32;
        self.buf[3..7].copy_from_slice(&size.to_be_bytes());
        self.buf
    }

    /// Writes a short or extended object-image length.
    fn write_length(&mut self, length: usize) {
        if length <= TTC_OBJ_MAX_SHORT_LENGTH {
            self.buf.push(length as u8);
        } else {
            self.buf.push(constants::TTC_LONG_LENGTH_INDICATOR);
            self.buf.extend((length as u32).to_be_bytes());
        }
    }

    /// Writes bytes prefixed by an object-image length.
    fn write_bytes_with_length(&mut self, bytes: &[u8]) {
        self.write_length(bytes.len());
        self.buf.extend_from_slice(bytes);
    }

    /// Writes a scalar in its TTC wire format, which carries a one-byte
    /// length that is also a valid short object-image length.
    fn write_ttc(&mut self, value: &dyn ToBuf, db_type: &'static DbType) {
        let mut buf = WriteBuffer::new();
        value.to_buf(&mut buf, db_type, true);
        self.buf.extend_from_slice(buf.get_buf());
    }

    /// Writes the attributes of an object.
    fn write_object_body(&mut self, object: &DbObject) {
        let attrs = object.object_type.attributes();
        for (metadata, value) in
            attrs.iter().zip(object.data.db_row().values())
        {
            self.write_value(&object.object_type, metadata, value);
        }
    }

    /// Writes the elements of a collection, with their keys for an
    /// associative array.
    fn write_collection_body(&mut self, collection: &DbCollection) {
        let metadata = collection.object_type.element_metadata().unwrap();
        let values = collection.values.values();
        self.buf.push(if collection.indices.is_some() {
            TTC_OBJ_HAS_INDEXES
        } else {
            0
        });
        self.write_length(values.len());
        for (pos, value) in values.iter().enumerate() {
            if let Some(indices) = &collection.indices {
                self.buf.extend((indices[pos] as u32).to_be_bytes());
            }
            self.write_value(&collection.object_type, metadata, value);
        }
    }

    /// Writes one attribute or element of a value of the `parent` type.
    fn write_value(
        &mut self,
        parent: &DbObjectType,
        metadata: &Metadata,
        value: &Option<DbValue>,
    ) {
        let typ = metadata.db_type();
        let Some(value) = value else {
            // only an object written inline is atomically NULL; one written
            // as an image of its own has a NULL length, and the server raises
            // ORA-00600 [kopp2ucoll233] otherwise
            let is_inline_object = typ == crate::DB_TYPE_OBJECT
                && metadata
                    .object_type()
                    .ok()
                    .flatten()
                    .is_some_and(|t| !has_own_image(parent, &t));
            self.buf.push(if is_inline_object {
                TTC_OBJ_ATOMIC_NULL
            } else {
                constants::TTC_NULL_LENGTH_INDICATOR
            });
            return;
        };
        match value {
            DbValue::String(v) => {
                if typ.csfrm == constants::CS_FORM_NCHAR {
                    let bytes: Vec<u8> =
                        v.encode_utf16().flat_map(u16::to_be_bytes).collect();
                    self.write_bytes_with_length(&bytes);
                } else {
                    self.write_bytes_with_length(v.as_bytes());
                }
            }
            DbValue::Bytes(v) => self.write_bytes_with_length(v),
            DbValue::Number(v) if typ == crate::DB_TYPE_BINARY_INTEGER => {
                // coerce() checked that the value fits
                let v: i32 = v.to_string().parse().unwrap_or_default();
                self.buf.push(4);
                self.buf.extend(v.to_be_bytes());
            }
            DbValue::Number(v) => self.write_ttc(v, typ),
            DbValue::BinaryDouble(v) => self.write_ttc(v, typ),
            DbValue::BinaryFloat(v) => self.write_ttc(v, typ),
            DbValue::Timestamp(v) => self.write_ttc(v, typ),
            DbValue::IntervalDS(v) => self.write_ttc(v, typ),
            DbValue::IntervalYM(v) => self.write_ttc(v, typ),
            DbValue::Boolean(v) => {
                self.buf.push(4);
                self.buf.extend(u32::from(*v).to_be_bytes());
            }
            DbValue::DbObject(v) => {
                if has_own_image(parent, &v.object_type) {
                    self.write_bytes_with_length(&v.packed_image());
                } else {
                    self.write_object_body(v);
                }
            }
            DbValue::DbCollection(v) => {
                self.write_bytes_with_length(&v.packed_image())
            }
            // coerce() admits no other value
            _ => self.buf.push(constants::TTC_NULL_LENGTH_INDICATOR),
        }
    }
}

/// Writes a top-level object bind value: the TTC envelope and the image.
fn write_bind_image(
    buf: &mut WriteBuffer,
    object_type: &DbObjectType,
    image: &[u8],
) {
    let mut toid = Vec::with_capacity(36);
    toid.extend([0x00, 0x22, TTC_OBJ_NON_NULL_OID, TTC_OBJ_HAS_EXTENT_OID]);
    toid.extend_from_slice(object_type.oid());
    toid.extend(TTC_OBJ_EXTENT_OID);
    buf.write_bytes_with_double_length(Some(&toid));
    buf.write_ub4(0); // OID
    buf.write_ub4(0); // snapshot
    buf.write_ub4(0); // version
    buf.write_ub4(image.len().try_into().unwrap());
    buf.write_ub4(TTC_OBJ_TOP_LEVEL); // flags
    buf.write_bytes_with_length(image);
}

/// Writes a NULL top-level object bind value.
fn write_null_bind(buf: &mut WriteBuffer) {
    buf.write_ub4(0); // TOID
    buf.write_ub4(0); // OID
    buf.write_ub4(0); // snapshot
    buf.write_ub2(0); // version
    buf.write_ub4(0); // image length
    buf.write_ub4(TTC_OBJ_TOP_LEVEL); // flags
}

impl ToBuf for DbObject {
    fn to_buf(&self, buf: &mut WriteBuffer, _: &'static DbType, _: bool) {
        write_bind_image(buf, &self.object_type, &self.packed_image());
    }

    fn to_buf_null(&self, buf: &mut WriteBuffer) {
        write_null_bind(buf);
    }
}

impl ToDbValue for DbObject {
    fn db_type(&self) -> &'static DbType {
        crate::DB_TYPE_OBJECT
    }

    fn db_object_type(&self) -> Option<Arc<DbObjectType>> {
        Some(Arc::clone(&self.object_type))
    }
}

impl ToBuf for DbCollection {
    fn to_buf(&self, buf: &mut WriteBuffer, _: &'static DbType, _: bool) {
        write_bind_image(buf, &self.object_type, &self.packed_image());
    }

    fn to_buf_null(&self, buf: &mut WriteBuffer) {
        write_null_bind(buf);
    }
}

impl ToDbValue for DbCollection {
    fn db_type(&self) -> &'static DbType {
        crate::DB_TYPE_OBJECT
    }

    fn db_object_type(&self) -> Option<Arc<DbObjectType>> {
        Some(Arc::clone(&self.object_type))
    }
}

/// Binding a type handle binds a NULL of that type: an IN NULL, or the
/// placeholder of an OUT bind (like binding a `&DbType` for scalars).
impl ToBuf for Arc<DbObjectType> {
    fn to_buf(&self, buf: &mut WriteBuffer, _: &'static DbType, _: bool) {
        write_null_bind(buf);
    }

    fn to_buf_null(&self, buf: &mut WriteBuffer) {
        write_null_bind(buf);
    }
}

impl ToDbValue for Arc<DbObjectType> {
    fn db_type(&self) -> &'static DbType {
        crate::DB_TYPE_OBJECT
    }

    fn db_object_type(&self) -> Option<Arc<DbObjectType>> {
        Some(Arc::clone(self))
    }
}
