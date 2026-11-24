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
// test_3700_dbobject.rs
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;

#[rstest]
/// Tests recursive collection discovery and cached object identity.
fn test_3700(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _address_type_guard = common::create_type(
        &conn,
        "test_3700_address_type",
        "object (street varchar2(100), zip number(5))",
    )?;
    let _person_type_guard = common::create_type(
        &conn,
        "test_3700_person_type",
        "object (id number, address test_3700_address_type)",
    )?;
    let _people_type_guard = common::create_type(
        &conn,
        "test_3700_people_type",
        "table of test_3700_person_type",
    )?;
    let _numbers_type_guard = common::create_type(
        &conn,
        "test_3700_numbers_type",
        "varray(10) of number",
    )?;

    // Discover the collection first: neither its element nor that element's
    // nested address type has been cached yet.
    let people_type = conn.get_type("TEST_3700_PEOPLE_TYPE")?;
    assert!(people_type.is_collection());
    let person_type =
        people_type.element_metadata().unwrap().object_type()?.unwrap();
    let person_attrs = person_type.attributes();
    assert_eq!(person_attrs.len(), 2);
    let nested_type = person_attrs[1].object_type()?.unwrap();
    let address_attrs = nested_type.attributes();
    assert_eq!(address_attrs.len(), 2);
    assert_eq!(address_attrs[0].name(), "STREET");
    assert_eq!(address_attrs[1].name(), "ZIP");
    assert_eq!(person_type, conn.get_type("TEST_3700_PERSON_TYPE")?);
    assert_eq!(nested_type, conn.get_type("TEST_3700_ADDRESS_TYPE")?);
    assert_eq!(people_type, conn.get_type("TEST_3700_PEOPLE_TYPE")?);

    let numbers_type = conn.get_type("TEST_3700_NUMBERS_TYPE")?;
    assert!(numbers_type.is_collection());
    let element_type = numbers_type.element_metadata().unwrap();
    assert_eq!(element_type.db_type(), oracledb::DB_TYPE_NUMBER);

    Ok(())
}

#[rstest]
/// Tests looking up an object type that does not exist.
fn test_3701(conn: oracledb::Connection) {
    let name = "A TYPE THAT DOES NOT EXIST";
    let error = conn.get_type(name).unwrap_err();
    assert_eq!(
        error.kind(),
        &oracledb::ErrorKind::InvalidDbObjectTypeName(name.to_string())
    );
}

#[rstest]
/// Tests metadata discovery for package records and associative-array elements.
fn test_3702(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let package_name = "TEST_3702_PACKAGE";
    let _package_guard = common::create_package(
        &conn,
        package_name,
        r#"
        type record_type is record
            (id pls_integer, label nvarchar2(30), enabled boolean);
        type record_list is table of record_type index by binary_integer;
        type number_list is table of number index by binary_integer;
        "#,
    )?;
    let schema: String =
        conn.query_row("select user from dual", &[])?.get(0)?;
    let records_type =
        conn.get_type(&format!("{package_name}.RECORD_LIST"))?;
    assert!(records_type.is_collection());
    assert_eq!(records_type.name(), "RECORD_LIST");
    assert_eq!(records_type.schema(), schema);
    assert_eq!(records_type.package_name(), Some(package_name));
    assert!(records_type.attributes().is_empty());
    let element_type = records_type.element_metadata().unwrap();
    assert_eq!(element_type.db_type(), oracledb::DB_TYPE_OBJECT);
    let record_type = element_type.object_type()?.unwrap();
    assert!(!record_type.is_collection());
    assert!(record_type.element_metadata().is_none());
    assert_eq!(record_type.schema(), schema);
    assert_eq!(record_type.package_name(), Some(package_name));
    let attrs = record_type.attributes();
    assert_eq!(attrs.len(), 3);
    assert_eq!(attrs[0].name(), "ID");
    assert_eq!(attrs[0].db_type(), oracledb::DB_TYPE_BINARY_INTEGER);
    assert_eq!(attrs[1].name(), "LABEL");
    assert_eq!(attrs[1].db_type(), oracledb::DB_TYPE_NVARCHAR);
    assert!(attrs[1].max_size() >= 30);
    assert_eq!(attrs[2].name(), "ENABLED");
    assert_eq!(attrs[2].db_type(), oracledb::DB_TYPE_BOOLEAN);
    assert_eq!(
        record_type,
        conn.get_type(&format!("{package_name}.RECORD_TYPE"))?
    );
    let numbers_type =
        conn.get_type(&format!("{package_name}.NUMBER_LIST"))?;
    assert_eq!(
        numbers_type.element_metadata().unwrap().db_type(),
        oracledb::DB_TYPE_NUMBER
    );
    assert_eq!(
        records_type,
        conn.get_type(&format!("{schema}.{package_name}.RECORD_LIST"))?
    );
    Ok(())
}

#[rstest]
/// Tests attribute metadata for object and row types.
fn test_3703(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _address_type_guard = common::create_type(
        &conn,
        "test_3703_address_type",
        "object (street varchar2(100), zip number(5))",
    )?;
    let _person_type_guard = common::create_type(
        &conn,
        "test_3703_person_type",
        "object (id number, address test_3703_address_type)",
    )?;
    let _people_type_guard = common::create_type(
        &conn,
        "test_3703_people_type",
        "table of test_3703_person_type",
    )?;
    let table_name = "TEST_3703_ALL_TYPES_TAB";
    let sql_type_name = "TEST_3703_ALL_TYPES".to_string();
    let row_type_name = format!("{table_name}%ROWTYPE");
    let _table_guard = common::create_table_with_options(
        &conn,
        table_name,
        r#"
            NUMBERVALUE number,
            STRINGVALUE varchar2(60 byte),
            FIXEDCHARVALUE char(10 byte),
            NSTRINGVALUE nvarchar2(60),
            NFIXEDCHARVALUE nchar(10),
            RAWVALUE raw(16),
            INTVALUE integer,
            SMALLINTVALUE smallint,
            REALVALUE real,
            DECIMALVALUE number(20, 6),
            DOUBLEPRECISIONVALUE double precision,
            FLOATVALUE float,
            BINARYFLOATVALUE binary_float,
            BINARYDOUBLEVALUE binary_double,
            DATEVALUE date,
            TIMESTAMPVALUE timestamp(3),
            TIMESTAMPTZVALUE timestamp(4) with time zone,
            TIMESTAMPLTZVALUE timestamp(5) with local time zone,
            INTERVALDSVALUE interval day(4) to second(2),
            INTERVALYMVALUE interval year(3) to month,
            CLOBVALUE clob,
            NCLOBVALUE nclob,
            BLOBVALUE blob,
            SUBOBJECTVALUE test_3703_address_type,
            SUBOBJECTARRAY test_3703_people_type,
            INVISIBLEVALUE number invisible
            "#,
        "nested table SUBOBJECTARRAY store as test_3703_all_types_tab_nt",
    )?;
    let _type_guard = common::create_type(
        &conn,
        &sql_type_name,
        r#"
            object (
                NUMBERVALUE number,
                STRINGVALUE varchar2(60 byte),
                FIXEDCHARVALUE char(10 byte),
                NSTRINGVALUE nvarchar2(60),
                NFIXEDCHARVALUE nchar(10),
                RAWVALUE raw(16),
                INTVALUE integer,
                SMALLINTVALUE smallint,
                REALVALUE real,
                DECIMALVALUE number(20, 6),
                DOUBLEPRECISIONVALUE double precision,
                FLOATVALUE float,
                BINARYFLOATVALUE binary_float,
                BINARYDOUBLEVALUE binary_double,
                DATEVALUE date,
                TIMESTAMPVALUE timestamp(3),
                TIMESTAMPTZVALUE timestamp(4) with time zone,
                TIMESTAMPLTZVALUE timestamp(5) with local time zone,
                INTERVALDSVALUE interval day(4) to second(2),
                INTERVALYMVALUE interval year(3) to month,
                CLOBVALUE clob,
                NCLOBVALUE nclob,
                BLOBVALUE blob,
                SUBOBJECTVALUE test_3703_address_type,
                SUBOBJECTARRAY test_3703_people_type
            )
            "#,
    )?;

    let schema: String =
        conn.query_row("select user from dual", &[])?.get(0)?;
    for name in [&row_type_name, &sql_type_name] {
        let typ = conn.get_type(name)?;
        assert_eq!(typ.name(), name);
        assert_eq!(typ.schema(), schema);
        assert_eq!(typ.package_name(), None);
        assert!(!typ.is_collection());
        assert!(typ.element_metadata().is_none());

        let expected = vec![
            ("NUMBERVALUE", oracledb::DB_TYPE_NUMBER, 0, -127, 0),
            ("STRINGVALUE", oracledb::DB_TYPE_VARCHAR, 0, 0, 60),
            ("FIXEDCHARVALUE", oracledb::DB_TYPE_CHAR, 0, 0, 10),
            ("NSTRINGVALUE", oracledb::DB_TYPE_NVARCHAR, 0, 0, 120),
            ("NFIXEDCHARVALUE", oracledb::DB_TYPE_NCHAR, 0, 0, 20),
            ("RAWVALUE", oracledb::DB_TYPE_RAW, 0, 0, 16),
            ("INTVALUE", oracledb::DB_TYPE_NUMBER, 38, 0, 0),
            ("SMALLINTVALUE", oracledb::DB_TYPE_NUMBER, 38, 0, 0),
            ("REALVALUE", oracledb::DB_TYPE_NUMBER, 63, -127, 0),
            ("DECIMALVALUE", oracledb::DB_TYPE_NUMBER, 20, 6, 0),
            ("DOUBLEPRECISIONVALUE", oracledb::DB_TYPE_NUMBER, 126, -127, 0),
            ("FLOATVALUE", oracledb::DB_TYPE_NUMBER, 126, -127, 0),
            ("BINARYFLOATVALUE", oracledb::DB_TYPE_BINARY_FLOAT, 0, 0, 0),
            ("BINARYDOUBLEVALUE", oracledb::DB_TYPE_BINARY_DOUBLE, 0, 0, 0),
            ("DATEVALUE", oracledb::DB_TYPE_DATE, 0, 0, 0),
            ("TIMESTAMPVALUE", oracledb::DB_TYPE_TIMESTAMP, 0, 3, 0),
            ("TIMESTAMPTZVALUE", oracledb::DB_TYPE_TIMESTAMP_TZ, 0, 4, 0),
            ("TIMESTAMPLTZVALUE", oracledb::DB_TYPE_TIMESTAMP_LTZ, 0, 5, 0),
            ("INTERVALDSVALUE", oracledb::DB_TYPE_INTERVAL_DS, 4, 2, 0),
            ("INTERVALYMVALUE", oracledb::DB_TYPE_INTERVAL_YM, 3, 0, 0),
            ("CLOBVALUE", oracledb::DB_TYPE_CLOB, 0, 0, 0),
            ("NCLOBVALUE", oracledb::DB_TYPE_NCLOB, 0, 0, 0),
            ("BLOBVALUE", oracledb::DB_TYPE_BLOB, 0, 0, 0),
            ("SUBOBJECTVALUE", oracledb::DB_TYPE_OBJECT, 0, 0, 0),
            ("SUBOBJECTARRAY", oracledb::DB_TYPE_OBJECT, 0, 0, 0),
        ];
        let attrs = typ.attributes();
        assert_eq!(attrs.len(), expected.len());
        for (metadata, (name, db_type, precision, scale, max_size)) in
            attrs.iter().zip(expected)
        {
            assert_eq!(
                (
                    metadata.name(),
                    metadata.db_type(),
                    metadata.precision(),
                    metadata.scale(),
                    metadata.max_size()
                ),
                (name, db_type, precision, scale, max_size),
                "attribute {name}"
            );
            match name {
                "SUBOBJECTVALUE" => {
                    let address_type = metadata.object_type()?.unwrap();
                    assert_eq!(address_type.attributes().len(), 2);
                    assert_eq!(
                        address_type,
                        conn.get_type("TEST_3703_ADDRESS_TYPE")?
                    );
                }
                "SUBOBJECTARRAY" => {
                    let collection_type = metadata.object_type()?.unwrap();
                    assert!(collection_type.is_collection());
                    assert!(collection_type.attributes().is_empty());
                    let element_type =
                        collection_type.element_metadata().unwrap();
                    let person_type = element_type.object_type()?.unwrap();
                    let person_attrs = person_type.attributes();
                    assert_eq!(person_attrs.len(), 2);
                    assert_eq!(
                        person_attrs[1]
                            .object_type()?
                            .unwrap()
                            .attributes()
                            .len(),
                        2
                    );
                    assert_eq!(
                        collection_type,
                        conn.get_type("TEST_3703_PEOPLE_TYPE")?
                    );
                    assert_eq!(
                        person_type,
                        conn.get_type("TEST_3703_PERSON_TYPE")?
                    );
                }
                _ => assert!(metadata.object_type()?.is_none()),
            }
        }
    }
    Ok(())
}

#[rstest]
/// Tests that retained metadata keeps nested types alive after the connection
/// closes.
fn test_3704(mut conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let child_type_guard = common::create_type(
        &conn,
        "test_3704_child",
        "object (value number)",
    )?;
    let parent_type_guard = common::create_type(
        &conn,
        "test_3704_parent",
        "object (child test_3704_child)",
    )?;
    let parent_type = conn.get_type("TEST_3704_PARENT")?;
    let attrs = parent_type.attributes();
    let child_type = attrs[0].object_type()?.unwrap();
    let child_ref = std::sync::Arc::downgrade(&child_type);
    drop(parent_type_guard);
    drop(child_type_guard);

    // Keep the client allocated to check that close releases the cache itself.
    let statement = conn.statement("select 1 from dual")?;
    conn.close()?;

    assert_eq!(parent_type.attributes()[0].name(), "CHILD");
    assert!(std::sync::Arc::ptr_eq(
        &attrs[0].object_type()?.unwrap(),
        &child_type,
    ));
    drop(child_type);
    assert_eq!(
        attrs[0].object_type()?.unwrap().attributes()[0].name(),
        "VALUE"
    );
    drop(statement);
    drop(parent_type);
    assert!(child_ref.upgrade().is_none());
    Ok(())
}

#[rstest]
/// Tests object type resolution from describe metadata without fetching
/// values.
fn test_3705(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _type_guard =
        common::create_type(&conn, "test_3705_type", "object (value number)")?;
    let mut statement = conn
        .statement("select test_3705_type(1) as obj from dual")?
        .build()?;
    statement.ensure_fully_parsed()?;
    let metadata = &statement.out_metadata()[0];
    assert_eq!(metadata.name(), "OBJ");
    let typ = metadata.object_type()?.unwrap();
    assert_eq!(typ.attributes()[0].name(), "VALUE");
    assert_eq!(typ, conn.get_type("TEST_3705_TYPE")?);

    let mut result = conn.execute_named(
        r#"
        begin
            open :cursor for
                select test_3705_type(1) as obj
                from dual;
        end;
        "#,
        &[("cursor", &oracledb::DB_TYPE_CURSOR)],
    )?;
    let cursor: oracledb::Cursor = result.out_bind_data().take("cursor")?;
    assert_eq!(cursor.columns()[0].object_type()?.unwrap(), typ);
    Ok(())
}

#[rstest]
/// Tests consistent XMLTYPE metadata for attributes, row types, elements and
/// columns.
fn test_3706(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _obj_type_guard = common::create_type(
        &conn,
        "test_3706_obj_type",
        "object (doc sys.xmltype)",
    )?;
    let _collection_type_guard = common::create_type(
        &conn,
        "test_3706_collection_type",
        "table of sys.xmltype",
    )?;
    let _table_guard =
        common::create_table(&conn, "test_3706_tab", "doc sys.xmltype")?;
    for name in ["TEST_3706_OBJ_TYPE", "TEST_3706_TAB%ROWTYPE"] {
        let typ = conn.get_type(name)?;
        let metadata = &typ.attributes()[0];
        assert_eq!(metadata.db_type(), oracledb::DB_TYPE_XMLTYPE);
        assert!(metadata.object_type()?.is_none());
    }
    let typ = conn.get_type("TEST_3706_COLLECTION_TYPE")?;
    let element = typ.element_metadata().unwrap();
    assert_eq!(element.db_type(), oracledb::DB_TYPE_XMLTYPE);
    assert!(element.object_type()?.is_none());

    let mut statement = conn
        .statement("select xmltype('<root/>') as doc from dual")?
        .build()?;
    statement.ensure_fully_parsed()?;
    let metadata = &statement.out_metadata()[0];
    assert_eq!(metadata.db_type(), oracledb::DB_TYPE_XMLTYPE);
    assert!(metadata.object_type()?.is_none());
    Ok(())
}

#[rstest]
/// Takes an object and reads its decoded nested values through shared
/// references.
fn test_3707(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let address_type_name = "TEST_3707_ADDRESS";
    let _address_type_guard = common::create_type(
        &conn,
        address_type_name,
        "object (street varchar2(100), zip number)",
    )?;
    let addresses_type_name = "TEST_3707_ADDRESSES";
    let _addresses_type_guard = common::create_type(
        &conn,
        addresses_type_name,
        &format!("table of {address_type_name}"),
    )?;
    let person_type_name = "TEST_3707_PERSON";
    let _person_type_guard = common::create_type(
        &conn,
        person_type_name,
        &format!(
            r#"
            object (
                id number,
                address {address_type_name},
                addresses {addresses_type_name}
            )
            "#,
        ),
    )?;
    let sql = format!(
        r#"
        select {person_type_name}(
            7,
            {address_type_name}('Main', 42),
            {addresses_type_name}({address_type_name}('Other', 43), null)
        ) from dual
        "#,
    );
    let mut row = conn.query_row(&sql, &[])?;
    let mut person: oracledb::DbObject = row.take(0)?;
    assert_eq!(person.get::<i32>("ID")?, 7);
    let address: oracledb::DbObject = person.take("ADDRESS")?;
    assert_eq!(address.get::<&str>("STREET")?, "Main");
    assert_eq!(address.get::<i32>("ZIP")?, 42);
    let addresses: oracledb::DbCollection = person.take("ADDRESSES")?;
    let mut elements: Vec<Option<oracledb::DbObject>> =
        addresses.try_into()?;
    //    let mut elements = addresses.to_vec::<Option<oracledb::DbObject>>()?;
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].take().unwrap().get::<i32>("ZIP")?, 43);
    assert!(elements[1].is_none());
    assert_eq!(person.object_type(), &conn.get_type(person_type_name)?);
    Ok(())
}

#[rstest]
/// Distinguishes null objects, null attributes, and empty collections.
fn test_3708(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3708_OBJ";
    let _obj_type_guard =
        common::create_type(&conn, obj_type_name, "object (value number)")?;
    let array_type_name = "TEST_3708_ARRAY";
    let _array_type_guard =
        common::create_type(&conn, array_type_name, "varray(10) of number")?;
    let sql = format!(
        r#"
        select
            cast(null as {obj_type_name}),
            {obj_type_name}(null),
            {array_type_name}(),
            {array_type_name}(1, null, 3)
        from dual
        "#,
    );
    let mut row = conn.query_row(&sql, &[])?;
    assert!(row.take::<Option<oracledb::DbObject>>(0)?.is_none());
    assert!(
        row.take::<oracledb::DbObject>(1)?.get::<Option<i32>>(0)?.is_none()
    );
    let empty_array: oracledb::DbCollection = row.take(2)?;
    assert!(TryInto::<Vec<i32>>::try_into(empty_array)?.is_empty());
    let array: oracledb::DbCollection = row.take(3)?;
    let values: Vec<Option<i32>> = array.try_into()?;
    assert_eq!(values, vec![Some(1), None, Some(3)]);
    Ok(())
}

#[rstest]
/// Fetches identical object values over several batches.
fn test_3709(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3709_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        "object (value number, text varchar2(20))",
    )?;
    let sql = format!(
        "select {}(7, 'same') from dual connect by level <= 9",
        obj_type_name
    );
    let cursor = conn
        .statement(&sql)?
        .prefetch_rows(0)
        .fetch_array_size(2)
        .build()?
        .query(&[])?;
    let mut count = 0;
    for row in cursor {
        let mut row = row?;
        let value: oracledb::DbObject = row.take(0)?;
        assert_eq!(value.get::<i32>(0)?, 7);
        assert_eq!(value.get::<String>(1)?, "same");
        count += 1;
    }
    assert_eq!(count, 9);
    Ok(())
}

#[rstest]
/// Reads LOB attributes after their object image has been decoded.
fn test_3710(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    use std::io::Read;
    let obj_type_name = "TEST_3710_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        "object (text clob, national_text nclob, data blob)",
    )?;
    let sql = format!(
        r#"
        select
            {}(
                to_clob('hello'),
                to_nclob('world'),
                to_blob(hextoraw('010203'))
            )
        from dual connect by level <= 2
        "#,
        obj_type_name,
    );
    let cursor = conn
        .statement(&sql)?
        .prefetch_rows(0)
        .fetch_array_size(1)
        .build()?
        .query(&[])?;
    for row_result in cursor {
        let mut row = row_result?;
        let mut value: oracledb::DbObject = row.take(0)?;
        let mut text: oracledb::Lob = value.take("TEXT")?;
        let mut result = String::new();
        text.read_to_string(&mut result).unwrap();
        assert_eq!(result, "hello");
        let mut national: oracledb::Lob = value.take("NATIONAL_TEXT")?;
        result.clear();
        national.read_to_string(&mut result).unwrap();
        assert_eq!(result, "world");
        let mut binary: oracledb::Lob = value.take("DATA")?;
        let mut bytes = Vec::new();
        binary.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
    }
    Ok(())
}

#[rstest]
/// Fetches the supported scalar attribute encodings.
fn test_3711(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3711_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        r#"
        object (
            n number,
            s varchar2(60),
            c char(10),
            ns nvarchar2(60),
            nc nchar(10),
            r raw(16),
            bf binary_float,
            bd binary_double,
            d date,
            ts timestamp,
            tz timestamp with time zone,
            ltz timestamp with local time zone
        )"#,
    )?;
    let mut row = conn.query_row(
        &format!(
            r#"
        select {}(
            12.5,
            'hello',
            'fixed',
            N'national',
            N'fixed',
            hextoraw('010203'),
            to_binary_float(1.25),
            to_binary_double(2.5),
            date '2020-01-02',
            timestamp '2021-01-02 03:04:05.123456',
            to_timestamp_tz('2022-01-02 03:04:05 +02:00',
                            'YYYY-MM-DD HH24:MI:SS TZH:TZM'),
            timestamp '2023-01-02 03:04:05'
        )
        from dual
        "#,
            obj_type_name,
        ),
        &[],
    )?;
    let value: oracledb::DbObject = row.take(0)?;
    assert_eq!(value.get::<f64>("N")?, 12.5);
    assert_eq!(value.get::<String>("S")?, "hello");
    assert_eq!(value.get::<String>("C")?, "fixed     ");
    assert_eq!(value.get::<String>("NS")?, "national");
    assert_eq!(value.get::<String>("NC")?, "fixed     ");
    assert_eq!(value.get::<Vec<u8>>("R")?, [1, 2, 3]);
    assert_eq!(value.get::<f32>("BF")?, 1.25);
    assert_eq!(value.get::<f64>("BD")?, 2.5);
    assert_eq!(value.get::<oracledb::OracleTimestamp>("D")?.year(), 2020);
    assert_eq!(
        value.get::<oracledb::OracleTimestamp>("TS")?.nanoseconds(),
        123456000
    );
    assert_eq!(value.get::<oracledb::OracleTimestamp>("TZ")?.year(), 2022);
    assert_eq!(value.get::<oracledb::OracleTimestamp>("LTZ")?.year(), 2023);
    Ok(())
}

#[rstest]
/// Discovers and decodes object and collection columns returned by a REF
/// CURSOR.
fn test_3712(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3712_OBJ";
    let _obj_type_guard =
        common::create_type(&conn, obj_type_name, "object (value number)")?;
    let array_type_name = "TEST_3712_ARRAY";
    let _array_type_guard =
        common::create_type(&conn, array_type_name, "varray(10) of number")?;
    let mut result = conn.execute_named(
        &format!(
            "begin open :cursor for select {}(7), {}(1, 2) from dual; end;",
            obj_type_name, array_type_name
        ),
        &[("cursor", &oracledb::DB_TYPE_CURSOR)],
    )?;
    let mut cursor: oracledb::Cursor =
        result.out_bind_data().take("cursor")?;
    let mut row = cursor.next().unwrap()?;
    let obj: oracledb::DbObject = row.take(0)?;
    assert_eq!(obj.get::<i32>("VALUE")?, 7);
    let array: oracledb::DbCollection = row.take(1)?;
    let values: Vec<i32> = array.try_into()?;
    assert_eq!(values, [1, 2]);
    assert!(cursor.next().is_none());
    Ok(())
}

#[rstest]
/// Fetches short and long string attributes using object-image lengths.
fn test_3713(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3713_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        "object (text_value varchar2(1000), trailing_value number)",
    )?;
    let sql = format!("select {}(:1, 42) from dual", obj_type_name);
    for length in [100, 256, 512] {
        let expected = "x".repeat(length);
        let mut row = conn.query_row(&sql, &[&expected])?;
        let value: oracledb::DbObject = row.take(0)?;
        assert_eq!(value.get::<String>("TEXT_VALUE")?, expected);
        assert_eq!(value.get::<i32>("TRAILING_VALUE")?, 42);
    }
    Ok(())
}

#[rstest]
/// Fetches nested images exceeding 255 bytes with only short scalar payloads.
fn test_3714(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3714_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        "object (first_value varchar2(100), second_value varchar2(100),
                 third_value varchar2(100), trailing_value number)",
    )?;
    let array_type_name = "TEST_3714_ARRAY";
    let _array_type_guard = common::create_type(
        &conn,
        array_type_name,
        &format!("table of {}", obj_type_name),
    )?;
    let sql = format!(
        "select {}({}(:1, :2, :3, 42), {}('after', 'nested', 'image', 99))
         from dual",
        array_type_name, obj_type_name, obj_type_name,
    );
    // Object elements in a collection have their own length-prefixed image.
    // Three 100-byte attributes make that image longer than 255 bytes without
    // requiring a long length prefix for any individual scalar attribute.
    for length in [10, 100] {
        let first = "a".repeat(length);
        let second = "b".repeat(length);
        let third = "c".repeat(length);
        let mut row = conn.query_row(&sql, &[&first, &second, &third])?;
        let collection: oracledb::DbCollection = row.take(0)?;
        let values: Vec<oracledb::DbObject> = collection.try_into()?;
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].get::<String>("FIRST_VALUE")?, first);
        assert_eq!(values[0].get::<String>("SECOND_VALUE")?, second);
        assert_eq!(values[0].get::<String>("THIRD_VALUE")?, third);
        assert_eq!(values[0].get::<i32>("TRAILING_VALUE")?, 42);
        assert_eq!(values[1].get::<String>("FIRST_VALUE")?, "after");
        assert_eq!(values[1].get::<String>("SECOND_VALUE")?, "nested");
        assert_eq!(values[1].get::<String>("THIRD_VALUE")?, "image");
        assert_eq!(values[1].get::<i32>("TRAILING_VALUE")?, 99);
    }
    Ok(())
}

#[rstest]
/// Binds an object built on the client IN and OUT with every scalar attribute
/// type that can be written in an image.
fn test_3715(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3715_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        r#"
        object (
            n number,
            s varchar2(60),
            c char(10),
            ns nvarchar2(60),
            nc nchar(10),
            r raw(16),
            bf binary_float,
            bd binary_double,
            b boolean,
            d date,
            ts timestamp,
            tz timestamp with time zone,
            ltz timestamp with local time zone,
            ids interval day to second,
            iym interval year to month
        )"#,
    )?;
    let obj_type = conn.get_type(obj_type_name)?;
    let mut obj = oracledb::DbObject::new(&obj_type)?;
    obj.set("N", "12.5".parse::<oracledb::OracleNumber>()?)?;
    obj.set("S", "ação")?;
    obj.set("C", "fixed")?;
    obj.set("NS", "national ação")?;
    obj.set("NC", "fixed")?;
    obj.set("R", vec![1u8, 2, 3])?;
    obj.set("BF", 1.25f32)?;
    obj.set("BD", 2.5f64)?;
    obj.set("B", true)?;
    obj.set("D", oracledb::OracleTimestamp::new_date(2020, 1, 2))?;
    let ts = oracledb::OracleTimestamp::new_timestamp(
        2021, 1, 2, 3, 4, 5, 123456000,
    );
    obj.set("TS", ts)?;
    let tz = oracledb::OracleTimestamp::new_utc_timestamp(
        2022, 1, 2, 3, 4, 5, 250000000,
    );
    obj.set("TZ", tz)?;
    let ltz = oracledb::OracleTimestamp::new_timestamp(2023, 1, 2, 3, 4, 5, 0);
    obj.set("LTZ", ltz)?;
    let ids = oracledb::OracleIntervalDS::new(1, 2, 3, 4, 500000000);
    obj.set("IDS", ids)?;
    let iym = oracledb::OracleIntervalYM::new(2, 3);
    obj.set("IYM", iym)?;
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                v {obj_type_name} := :value_in;
            begin
                :summary := to_char(v.n) || '|' || v.s || '|' || v.c || '|'
                    || v.ns || '|' || v.nc || '|' || rawtohex(v.r) || '|'
                    || to_char(v.bf, 'FM0.00') || '|'
                    || to_char(v.bd, 'FM0.00') || '|'
                    || case when v.b then 'true' else 'false' end || '|'
                    || to_char(v.d, 'YYYY-MM-DD HH24:MI:SS') || '|'
                    || to_char(v.ts, 'YYYY-MM-DD HH24:MI:SS.FF6') || '|'
                    || to_char(v.tz, 'YYYY-MM-DD HH24:MI:SS.FF3 TZH:TZM')
                    || '|' || to_char(v.ltz, 'YYYY-MM-DD HH24:MI:SS') || '|'
                    || to_char(v.ids) || '|' || to_char(v.iym);
                :value_out := v;
            end;"#
        ),
        &[
            ("value_in", &obj),
            ("summary", &oracledb::DB_TYPE_VARCHAR),
            ("value_out", &obj_type),
        ],
    )?;
    let mut out = result.out_bind_data();
    // CHAR and NCHAR attributes are stored as given, without blank-padding
    assert_eq!(
        out.get::<String>("summary")?,
        "12.5|ação|fixed|national ação|fixed|010203|1.25|2.50|true|\
         2020-01-02 00:00:00|2021-01-02 03:04:05.123456|\
         2022-01-02 03:04:05.250 +00:00|2023-01-02 03:04:05|\
         +01 02:03:04.500000|+02-03"
    );
    let value: oracledb::DbObject = out.take("value_out")?;
    assert_eq!(value.get::<f64>("N")?, 12.5);
    assert_eq!(value.get::<String>("S")?, "ação");
    assert_eq!(value.get::<String>("C")?, "fixed");
    assert_eq!(value.get::<String>("NS")?, "national ação");
    assert_eq!(value.get::<String>("NC")?, "fixed");
    assert_eq!(value.get::<Vec<u8>>("R")?, [1, 2, 3]);
    assert_eq!(value.get::<f32>("BF")?, 1.25);
    assert_eq!(value.get::<f64>("BD")?, 2.5);
    assert!(value.get::<bool>("B")?);
    assert_eq!(
        value.get::<oracledb::OracleTimestamp>("D")?,
        oracledb::OracleTimestamp::new_date(2020, 1, 2)
    );
    assert_eq!(value.get::<oracledb::OracleTimestamp>("TS")?, ts);
    assert_eq!(value.get::<oracledb::OracleTimestamp>("TZ")?, tz);
    assert_eq!(value.get::<oracledb::OracleTimestamp>("LTZ")?, ltz);
    assert_eq!(value.get::<oracledb::OracleIntervalDS>("IDS")?, ids);
    assert_eq!(value.get::<oracledb::OracleIntervalYM>("IYM")?, iym);
    Ok(())
}

#[rstest]
/// Distinguishes an atomically NULL object, an object whose attributes are
/// all NULL, a NULL collection, an empty collection and NULL elements, both
/// IN and OUT.
fn test_3716(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let point_type_name = "TEST_3716_POINT";
    let _point_type_guard = common::create_type(
        &conn,
        point_type_name,
        "object (x number, label varchar2(10))",
    )?;
    let points_type_name = "TEST_3716_POINTS";
    let _points_type_guard = common::create_type(
        &conn,
        points_type_name,
        &format!("table of {point_type_name}"),
    )?;
    let point_type = conn.get_type(point_type_name)?;
    let points_type = conn.get_type(points_type_name)?;
    let describe_point = format!(
        r#"
        declare
            v {point_type_name} := :value;
        begin
            if v is null then
                :result := 'atomic null';
            else
                :result := nvl(to_char(v.x), '-') || '|' || nvl(v.label, '-');
            end if;
        end;"#
    );
    let describe = |value: &dyn oracledb::ToDbValue| {
        conn.execute_named(
            &describe_point,
            &[("value", value), ("result", &oracledb::DB_TYPE_VARCHAR)],
        )?
        .out_bind_data()
        .get::<String>("result")
    };
    assert_eq!(describe(&point_type)?, "atomic null");
    assert_eq!(describe(&oracledb::DbObject::new(&point_type)?)?, "-|-");

    let describe_points = format!(
        r#"
        declare
            v {points_type_name} := :value;
            s varchar2(100);
        begin
            if v is null then
                :result := 'null';
                return;
            end if;
            s := v.count || ':';
            for i in 1 .. v.count loop
                if v(i) is null then
                    s := s || 'null,';
                else
                    s := s || nvl(to_char(v(i).x), '-') || ',';
                end if;
            end loop;
            :result := s;
        end;"#
    );
    let describe = |value: &dyn oracledb::ToDbValue| {
        conn.execute_named(
            &describe_points,
            &[("value", value), ("result", &oracledb::DB_TYPE_VARCHAR)],
        )?
        .out_bind_data()
        .get::<String>("result")
    };
    assert_eq!(describe(&points_type)?, "null");
    assert_eq!(describe(&oracledb::DbCollection::new(&points_type)?)?, "0:");
    let mut points = oracledb::DbCollection::new(&points_type)?;
    let mut point = oracledb::DbObject::new(&point_type)?;
    point.set("X", 1)?;
    points.push(point)?;
    points.push(None::<oracledb::DbObject>)?;
    points.push(oracledb::DbObject::new(&point_type)?)?;
    assert_eq!(points.len(), 3);
    assert_eq!(describe(&points)?, "3:1,null,-,");

    let mut result = conn.execute_named(
        &format!(
            r#"
            begin
                :null_point := null;
                :empty_points := {points_type_name}();
                :null_points := null;
                :sparse_points :=
                    {points_type_name}(null, {point_type_name}(null, null));
            end;"#
        ),
        &[
            ("null_point", &point_type),
            ("empty_points", &points_type),
            ("null_points", &points_type),
            ("sparse_points", &points_type),
        ],
    )?;
    let mut out = result.out_bind_data();
    assert!(out.take::<Option<oracledb::DbObject>>("null_point")?.is_none());
    let empty: oracledb::DbCollection = out.take("empty_points")?;
    assert!(empty.is_empty());
    assert!(
        out.take::<Option<oracledb::DbCollection>>("null_points")?.is_none()
    );
    let sparse: oracledb::DbCollection = out.take("sparse_points")?;
    let sparse: Vec<Option<oracledb::DbObject>> = sparse.try_into()?;
    assert_eq!(sparse.len(), 2);
    assert!(sparse[0].is_none());
    let all_null = sparse[1].as_ref().unwrap();
    assert_eq!(all_null.get::<Option<f64>>("X")?, None);
    assert_eq!(all_null.get::<Option<String>>("LABEL")?, None);
    Ok(())
}

#[rstest]
/// Binds nested tables and VARRAYs IN, OUT and IN OUT, including
/// collections of objects, collections of collections and TABLE(:ids).
fn test_3717(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let numbers_type_name = "TEST_3717_NUMBERS";
    let _numbers_type_guard =
        common::create_type(&conn, numbers_type_name, "table of number")?;
    let strings_type_name = "TEST_3717_STRINGS";
    let _strings_type_guard = common::create_type(
        &conn,
        strings_type_name,
        "varray(3) of varchar2(10)",
    )?;
    let matrix_type_name = "TEST_3717_MATRIX";
    let _matrix_type_guard = common::create_type(
        &conn,
        matrix_type_name,
        &format!("table of {numbers_type_name}"),
    )?;
    let numbers_type = conn.get_type(numbers_type_name)?;
    let strings_type = conn.get_type(strings_type_name)?;
    let matrix_type = conn.get_type(matrix_type_name)?;

    // TABLE(:ids) with 10,000 elements, alone and in a join
    let mut ids = oracledb::DbCollection::new(&numbers_type)?;
    for id in 1..=10_000 {
        ids.push(id)?;
    }
    let row = conn.query_row(
        "select count(*), sum(column_value) from table(:1)",
        &[&ids],
    )?;
    assert_eq!(row.get::<i64>(0)?, 10_000);
    assert_eq!(row.get::<i64>(1)?, 50_005_000);
    let row = conn.query_row(
        "select count(*) from table(:1) a join table(:2) b
         on a.column_value = b.column_value * 2",
        &[&ids, &ids],
    )?;
    assert_eq!(row.get::<i64>(0)?, 5_000);

    // OUT, then IN OUT
    let mut result = conn.execute_named(
        &format!(
            "begin :numbers := {numbers_type_name}(3, null, 1); \
             :strings := {strings_type_name}('a', 'ação'); end;"
        ),
        &[("numbers", &numbers_type), ("strings", &strings_type)],
    )?;
    let mut out = result.out_bind_data();
    let numbers: oracledb::DbCollection = out.take("numbers")?;
    let numbers: Vec<Option<i32>> = numbers.try_into()?;
    assert_eq!(numbers, [Some(3), None, Some(1)]);
    let mut strings: oracledb::DbCollection = out.take("strings")?;
    strings.push("x")?;
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                v {strings_type_name} := :strings;
            begin
                v(1) := upper(v(1));
                :strings := v;
            end;"#
        ),
        &[("strings", &strings)],
    )?;
    let strings: oracledb::DbCollection =
        result.out_bind_data().take("strings")?;
    let strings: Vec<String> = strings.try_into()?;
    assert_eq!(strings, ["A", "ação", "x"]);

    // a VARRAY past its limit is refused by the database, which keeps the
    // session usable
    let mut strings = oracledb::DbCollection::new(&strings_type)?;
    for value in ["a", "b", "c", "d"] {
        strings.push(value)?;
    }
    let error = conn
        .execute_named(
            &format!("declare v {strings_type_name} := :v; begin null; end;"),
            &[("v", &strings)],
        )
        .unwrap_err();
    assert_eq!(error.db_error_code(), Some(22165));
    let row = conn.query_row("select 1 from dual", &[])?;
    assert_eq!(row.get::<i32>(0)?, 1);

    // a collection of collections
    let mut matrix = oracledb::DbCollection::new(&matrix_type)?;
    for values in [&[1, 2][..], &[], &[3]] {
        let mut row = oracledb::DbCollection::new(&numbers_type)?;
        for value in values {
            row.push(*value)?;
        }
        matrix.push(row)?;
    }
    matrix.push(None::<oracledb::DbCollection>)?;
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                v {matrix_type_name} := :matrix;
                s varchar2(100);
            begin
                for i in 1 .. v.count loop
                    if v(i) is null then
                        s := s || 'null;';
                    else
                        for j in 1 .. v(i).count loop
                            s := s || v(i)(j) || ',';
                        end loop;
                        s := s || ';';
                    end if;
                end loop;
                :summary := s;
                v.extend;
                v(v.last) := {numbers_type_name}(4, 5);
                :matrix := v;
            end;"#
        ),
        &[("matrix", &matrix), ("summary", &oracledb::DB_TYPE_VARCHAR)],
    )?;
    let mut out = result.out_bind_data();
    assert_eq!(out.get::<String>("summary")?, "1,2,;;3,;null;");
    let matrix: oracledb::DbCollection = out.take("matrix")?;
    let rows: Vec<Option<oracledb::DbCollection>> = matrix.try_into()?;
    let rows = rows
        .into_iter()
        .map(|row| row.map(|row| row.try_into()).transpose())
        .collect::<Result<Vec<Option<Vec<i32>>>, _>>()?;
    assert_eq!(
        rows,
        [
            Some(vec![1, 2]),
            Some(vec![]),
            Some(vec![3]),
            None,
            Some(vec![4, 5])
        ]
    );
    Ok(())
}

#[rstest]
/// Binds INDEX BY PLS_INTEGER associative arrays IN, OUT and IN OUT, with
/// sparse and negative keys, and records with BINARY_INTEGER and BOOLEAN
/// attributes.
fn test_3718(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let package_name = "TEST_3718_PACKAGE";
    let _package_guard = common::create_package(
        &conn,
        package_name,
        r#"
        type number_map is table of number index by pls_integer;
        type record_type is record
            (id pls_integer, label varchar2(10), enabled boolean);
        type record_map is table of record_type index by binary_integer;
        "#,
    )?;
    let map_type = conn.get_type(&format!("{package_name}.NUMBER_MAP"))?;
    let record_type = conn.get_type(&format!("{package_name}.RECORD_TYPE"))?;
    let records_type = conn.get_type(&format!("{package_name}.RECORD_MAP"))?;
    assert!(map_type.is_associative_array());
    let describe_map = format!(
        r#"
        declare
            m {package_name}.number_map := :map;
            k pls_integer := m.first;
            s varchar2(200);
        begin
            while k is not null loop
                s := s || k || '=' || m(k) || ',';
                k := m.next(k);
            end loop;
            :summary := m.count || ':' || s;
            m(15) := 99;
            m.delete(10);
            :map := m;
        end;"#
    );

    // keys are kept in order and a key set twice keeps its last value
    let mut map = oracledb::DbCollection::new(&map_type)?;
    map.insert(20, 2)?;
    map.insert(10, 7)?;
    map.insert(-5, 3.5)?;
    map.insert(10, 1)?;
    assert_eq!(map.keys(), Some(&[-5, 10, 20][..]));
    let mut result = conn.execute_named(
        &describe_map,
        &[("map", &map), ("summary", &oracledb::DB_TYPE_VARCHAR)],
    )?;
    let mut out = result.out_bind_data();
    assert_eq!(out.get::<String>("summary")?, "3:-5=3.5,10=1,20=2,");
    let map: oracledb::DbCollection = out.take("map")?;
    assert_eq!(map.keys(), Some(&[-5, 15, 20][..]));
    let values: Vec<f64> = map.try_into()?;
    assert_eq!(values, [3.5, 99.0, 2.0]);

    // an empty associative array, IN and OUT
    let mut result = conn.execute_named(
        &describe_map,
        &[
            ("map", &oracledb::DbCollection::new(&map_type)?),
            ("summary", &oracledb::DB_TYPE_VARCHAR),
        ],
    )?;
    let out = result.out_bind_data();
    assert_eq!(out.get::<String>("summary")?, "0:");
    let mut result = conn.execute_named(
        &format!("declare m {package_name}.number_map; begin :map := m; end;"),
        &[("map", &map_type)],
    )?;
    let map: oracledb::DbCollection = result.out_bind_data().take("map")?;
    assert_eq!(map.keys(), Some(&[][..]));

    // records with BINARY_INTEGER and BOOLEAN attributes
    let mut records = oracledb::DbCollection::new(&records_type)?;
    for (key, id, label, enabled) in
        [(-2, i32::MIN, "first", true), (40, i32::MAX, "last", false)]
    {
        let mut record = oracledb::DbObject::new(&record_type)?;
        record.set("ID", id)?;
        record.set("LABEL", label)?;
        record.set("ENABLED", enabled)?;
        records.insert(key, record)?;
    }
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                m {package_name}.record_map := :records;
                k pls_integer := m.first;
                s varchar2(200);
            begin
                while k is not null loop
                    s := s || k || '=' || m(k).id || '/' || m(k).label || '/'
                        || case when m(k).enabled then 'Y' else 'N' end
                        || ',';
                    k := m.next(k);
                end loop;
                :summary := s;
                m(-2).id := m(-2).id + 1;
                :records := m;
            end;"#
        ),
        &[("records", &records), ("summary", &oracledb::DB_TYPE_VARCHAR)],
    )?;
    let mut out = result.out_bind_data();
    assert_eq!(
        out.get::<String>("summary")?,
        "-2=-2147483648/first/Y,40=2147483647/last/N,"
    );
    let records: oracledb::DbCollection = out.take("records")?;
    assert_eq!(records.keys(), Some(&[-2, 40][..]));
    let records: Vec<oracledb::DbObject> = records.try_into()?;
    assert_eq!(records[0].get::<i32>("ID")?, i32::MIN + 1);
    assert!(!records[1].get::<bool>("ENABLED")?);
    Ok(())
}

#[rstest]
/// Binds objects IN OUT, and binds back objects and collections that were
/// fetched.
fn test_3719(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let point_type_name = "TEST_3719_POINT";
    let _point_type_guard = common::create_type(
        &conn,
        point_type_name,
        "object (x number, y number, label varchar2(10))",
    )?;
    let vertices_type_name = "TEST_3719_VERTICES";
    let _vertices_type_guard = common::create_type(
        &conn,
        vertices_type_name,
        &format!("varray(100) of {point_type_name}"),
    )?;
    let polygon_type_name = "TEST_3719_POLYGON";
    let _polygon_type_guard = common::create_type(
        &conn,
        polygon_type_name,
        &format!("object (name varchar2(10), vertices {vertices_type_name})"),
    )?;
    let move_point = format!(
        r#"
        declare
            v {point_type_name} := :point;
        begin
            v.x := v.x + 10;
            v.y := v.y + 20;
            :point := v;
        end;"#
    );
    let point_type = conn.get_type(point_type_name)?;
    let mut point = oracledb::DbObject::new(&point_type)?;
    point.set("X", 1)?;
    point.set("Y", 2)?;
    point.set("LABEL", "ação")?;
    let mut result = conn.execute_named(&move_point, &[("point", &point)])?;
    let point: oracledb::DbObject = result.out_bind_data().take("point")?;
    assert_eq!(point.get::<i32>("X")?, 11);
    assert_eq!(point.get::<i32>("Y")?, 22);
    assert_eq!(point.get::<String>("LABEL")?, "ação");

    // a fetched object, changed on the client, binds back
    let mut row = conn.query_row(
        &format!(
            "select {polygon_type_name}('tri', {vertices_type_name}(
                {point_type_name}(0, 0, 'a'),
                {point_type_name}(1, 0, 'b'),
                {point_type_name}(0, 1, 'c'))) from dual"
        ),
        &[],
    )?;
    let mut polygon: oracledb::DbObject = row.take(0)?;
    polygon.set("NAME", "tri+")?;
    let mut vertices: oracledb::DbCollection = polygon.take("VERTICES")?;
    let mut vertex = oracledb::DbObject::new(&point_type)?;
    vertex.set("X", 9)?;
    vertex.set("Y", 9)?;
    vertex.set("LABEL", "new")?;
    vertices.push(vertex)?;
    polygon.set("VERTICES", vertices)?;
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                v {polygon_type_name} := :polygon;
            begin
                :summary := v.name || ':' || v.vertices.count || ':'
                    || v.vertices(v.vertices.last).label;
                v.vertices(1).label := 'moved';
                :polygon := v;
            end;"#
        ),
        &[("polygon", &polygon), ("summary", &oracledb::DB_TYPE_VARCHAR)],
    )?;
    let mut out = result.out_bind_data();
    assert_eq!(out.get::<String>("summary")?, "tri+:4:new");
    let mut polygon: oracledb::DbObject = out.take("polygon")?;
    let vertices: oracledb::DbCollection = polygon.take("VERTICES")?;
    let vertices: Vec<oracledb::DbObject> = vertices.try_into()?;
    assert_eq!(vertices.len(), 4);
    assert_eq!(vertices[0].get::<String>("LABEL")?, "moved");
    assert_eq!(vertices[3].get::<i32>("X")?, 9);
    Ok(())
}

#[rstest]
/// Reports each failure to build an object or a collection with its own
/// error kind, before any round trip.
fn test_3720(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let point_type_name = "TEST_3720_POINT";
    let _point_type_guard = common::create_type(
        &conn,
        point_type_name,
        "object (x number, label varchar2(5), ts timestamp, code nchar(2))",
    )?;
    let other_type_name = "TEST_3720_OTHER";
    let _other_type_guard =
        common::create_type(&conn, other_type_name, "object (x number)")?;
    let points_type_name = "TEST_3720_POINTS";
    let _points_type_guard = common::create_type(
        &conn,
        points_type_name,
        &format!("table of {point_type_name}"),
    )?;
    let package_name = "TEST_3720_PACKAGE";
    let _package_guard = common::create_package(
        &conn,
        package_name,
        r#"
        type number_map is table of number index by pls_integer;
        type record_type is record (id pls_integer);
        "#,
    )?;
    let point_type = conn.get_type(point_type_name)?;
    let other_type = conn.get_type(other_type_name)?;
    let points_type = conn.get_type(points_type_name)?;
    let map_type = conn.get_type(&format!("{package_name}.NUMBER_MAP"))?;
    let record_type = conn.get_type(&format!("{package_name}.RECORD_TYPE"))?;

    let is_wrong_kind = |error: oracledb::Error| {
        matches!(error.kind(), oracledb::ErrorKind::WrongDbObjectTypeKind(..))
    };
    assert!(is_wrong_kind(oracledb::DbObject::new(&points_type).unwrap_err()));
    assert!(is_wrong_kind(
        oracledb::DbCollection::new(&point_type).unwrap_err()
    ));
    let mut map = oracledb::DbCollection::new(&map_type)?;
    assert!(is_wrong_kind(map.push(1).unwrap_err()));
    let mut points = oracledb::DbCollection::new(&points_type)?;
    assert!(is_wrong_kind(points.insert(1, None::<i32>).unwrap_err()));

    let cannot_store = |error: oracledb::Error| match error.kind() {
        oracledb::ErrorKind::CannotStoreDbObjectValue(value, target, _) => {
            format!("{value} in {target}")
        }
        _ => panic!("unexpected error: {error}"),
    };
    let mut point = oracledb::DbObject::new(&point_type)?;
    assert_eq!(
        cannot_store(point.set("X", "1").unwrap_err()),
        "a string value in attribute X"
    );
    assert_eq!(
        cannot_store(point.set("X", f64::NAN).unwrap_err()),
        "the value NaN in attribute X"
    );
    assert_eq!(
        cannot_store(point.set("LABEL", "toolong").unwrap_err()),
        "a value of 7 bytes (the maximum is 5) in attribute LABEL"
    );
    assert_eq!(
        cannot_store(point.set("CODE", "abc").unwrap_err()),
        "a value of 6 bytes (the maximum is 4) in attribute CODE"
    );
    assert_eq!(
        cannot_store(point.set("TS", true).unwrap_err()),
        "a boolean value in attribute TS"
    );
    assert_eq!(
        cannot_store(
            points.push(oracledb::DbObject::new(&other_type)?).unwrap_err()
        ),
        format!("a {} in a collection element", other_type.fqn())
    );
    assert_eq!(
        cannot_store(map.insert(1, "x").unwrap_err()),
        "a string value in a collection element"
    );
    let mut record = oracledb::DbObject::new(&record_type)?;
    assert_eq!(
        cannot_store(record.set("ID", 2_147_483_648i64).unwrap_err()),
        "the value 2147483648 in attribute ID"
    );

    // a DB_TYPE_OBJECT bind without its type never reaches the server, where
    // it would be malformed and eventually drop the session
    for _ in 0..3 {
        let error = conn
            .execute_named(
                "begin :value := null; end;",
                &[("value", &oracledb::DB_TYPE_OBJECT)],
            )
            .unwrap_err();
        assert_eq!(
            error.kind(),
            &oracledb::ErrorKind::MissingDbObjectType("VALUE".to_string())
        );
    }
    let row = conn.query_row("select 1 from dual", &[])?;
    assert_eq!(row.get::<i32>(0)?, 1);
    Ok(())
}

#[rstest]
/// Refuses values for attributes whose type cannot be written in an image,
/// and binds the object when those attributes are NULL.
fn test_3721(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let obj_type_name = "TEST_3721_OBJ";
    let _obj_type_guard = common::create_type(
        &conn,
        obj_type_name,
        "object (id number, text clob, data blob)",
    )?;
    let obj_type = conn.get_type(obj_type_name)?;
    let mut obj = oracledb::DbObject::new(&obj_type)?;
    for (name, error) in [
        ("TEXT", obj.set("TEXT", "text").unwrap_err()),
        ("DATA", obj.set("DATA", vec![1u8]).unwrap_err()),
    ] {
        assert!(
            matches!(
                error.kind(),
                oracledb::ErrorKind::UnsupportedDbObjectAttributeType(target, _)
                    if *target == format!("attribute {name}")
            ),
            "{error}"
        );
    }
    obj.set("ID", 1)?;
    obj.set("TEXT", None::<String>)?;
    let mut result = conn.execute_named(
        &format!(
            r#"
            declare
                v {obj_type_name} := :value;
            begin
                :summary := v.id || ':'
                    || case when v.text is null and v.data is null
                        then 'null' end;
            end;"#
        ),
        &[("value", &obj), ("summary", &oracledb::DB_TYPE_VARCHAR)],
    )?;
    assert_eq!(result.out_bind_data().get::<String>("summary")?, "1:null");
    Ok(())
}

#[rstest]
/// Reads and binds objects of NOT FINAL types, and fails on instances of
/// subtypes, on read and on bind.
fn test_3722(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let base_type_name = "TEST_3722_BASE";
    let _base_type_guard = common::create_type(
        &conn,
        base_type_name,
        "object (a number, s varchar2(10)) not final",
    )?;
    let sub_type_name = "TEST_3722_SUB";
    let _sub_type_guard = common::create_subtype(
        &conn,
        sub_type_name,
        &format!("under {base_type_name} (b varchar2(10))"),
    )?;
    let holder_type_name = "TEST_3722_HOLDER";
    let _holder_type_guard = common::create_type(
        &conn,
        holder_type_name,
        &format!("object (o {base_type_name}, n number)"),
    )?;
    let is_subtype_error =
        |result: Result<oracledb::DbObject, oracledb::Error>| {
            matches!(
                result.map(|_| ()).unwrap_err().kind(),
                oracledb::ErrorKind::UnsupportedDbObjectSubtype(_)
            )
        };
    let fetch = |sql: &str| {
        conn.query_row(sql, &[])
            .and_then(|mut row| row.take::<oracledb::DbObject>(0))
    };

    // objects of a NOT FINAL type, alone and as an attribute
    let base = fetch(&format!("select {base_type_name}(1, 'x') from dual"))?;
    assert_eq!(base.get::<i32>("A")?, 1);
    assert_eq!(base.get::<String>("S")?, "x");
    let mut holder = fetch(&format!(
        "select {holder_type_name}({base_type_name}(2, 'y'), 5) from dual"
    ))?;
    let inner: oracledb::DbObject = holder.take("O")?;
    assert_eq!(inner.get::<i32>("A")?, 2);
    assert_eq!(inner.get::<String>("S")?, "y");
    assert_eq!(holder.get::<i32>("N")?, 5);
    let mut holder =
        fetch(&format!("select {holder_type_name}(null, 6) from dual"))?;
    assert!(holder.take::<Option<oracledb::DbObject>>("O")?.is_none());
    assert_eq!(holder.get::<i32>("N")?, 6);

    // instances of a subtype, alone, as a supertype and as an attribute
    assert!(is_subtype_error(fetch(&format!(
        "select {sub_type_name}(3, 'z', 'w') from dual"
    ))));
    assert!(is_subtype_error(fetch(&format!(
        "select treat({sub_type_name}(3, 'z', 'w') as {base_type_name}) \
         from dual"
    ))));
    assert!(is_subtype_error(fetch(&format!(
        "select {holder_type_name}({sub_type_name}(3, 'z', 'w'), 7) from dual"
    ))));
    let row = conn.query_row("select 1 from dual", &[])?;
    assert_eq!(row.get::<i32>(0)?, 1);
    let sub_type = conn.get_type(sub_type_name)?;
    assert!(is_subtype_error(oracledb::DbObject::new(&sub_type)));

    // binds of objects of a NOT FINAL type, alone and as an attribute
    let base_type = conn.get_type(base_type_name)?;
    let holder_type = conn.get_type(holder_type_name)?;
    let mut base = oracledb::DbObject::new(&base_type)?;
    base.set("A", 8)?;
    base.set("S", "q")?;
    let mut holder = oracledb::DbObject::new(&holder_type)?;
    holder.set("O", base)?;
    holder.set("N", 9)?;
    let mut empty_holder = oracledb::DbObject::new(&holder_type)?;
    empty_holder.set("N", 10)?;
    let sql = format!(
        r#"
        declare
            v {holder_type_name} := :holder;
        begin
            if v.o is null then
                :summary := 'null:' || v.n;
            else
                :summary := v.o.a || '/' || v.o.s || ':' || v.n;
            end if;
            :holder := v;
        end;"#
    );
    for (value, expected) in [(holder, "8/q:9"), (empty_holder, "null:10")] {
        let mut result = conn.execute_named(
            &sql,
            &[("holder", &value), ("summary", &oracledb::DB_TYPE_VARCHAR)],
        )?;
        let mut out = result.out_bind_data();
        assert_eq!(out.get::<String>("summary")?, expected);
        let mut holder: oracledb::DbObject = out.take("holder")?;
        let inner: Option<oracledb::DbObject> = holder.take("O")?;
        assert_eq!(inner.is_none(), expected.starts_with("null"));
    }
    Ok(())
}

#[rstest]
/// Binds objects and collections in every row of a batch.
fn test_3723(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let point_type_name = "TEST_3723_POINT";
    let _point_type_guard = common::create_type(
        &conn,
        point_type_name,
        "object (x number, label varchar2(10))",
    )?;
    let numbers_type_name = "TEST_3723_NUMBERS";
    let _numbers_type_guard =
        common::create_type(&conn, numbers_type_name, "varray(10) of number")?;
    let other_type_name = "TEST_3723_OTHER";
    let _other_type_guard =
        common::create_type(&conn, other_type_name, "object (x number)")?;
    let table_name = "TEST_3723_TAB";
    let _table_guard = common::create_table(
        &conn,
        table_name,
        &format!(
            "id number, point {point_type_name}, numbers {numbers_type_name}"
        ),
    )?;
    let point_type = conn.get_type(point_type_name)?;
    let numbers_type = conn.get_type(numbers_type_name)?;
    let other_type = conn.get_type(other_type_name)?;
    let point = |x: i32, label: &str| -> Result<_, oracledb::Error> {
        let mut point = oracledb::DbObject::new(&point_type)?;
        point.set("X", x)?;
        point.set("LABEL", label)?;
        Ok(point)
    };
    let numbers = |values: &[i32]| -> Result<_, oracledb::Error> {
        let mut numbers = oracledb::DbCollection::new(&numbers_type)?;
        for value in values {
            numbers.push(*value)?;
        }
        Ok(numbers)
    };
    let (point1, point3) = (point(1, "one")?, point(3, "three")?);
    let (numbers1, numbers3) = (numbers(&[1, 2])?, numbers(&[])?);
    let sql = format!("insert into {table_name} values (:1, :2, :3)");
    let rows: &[&[&dyn oracledb::ToDbValue]] = &[
        &[&1, &point1, &numbers1],
        &[&2, &point_type, &numbers_type],
        &[&3, &point3, &numbers3],
    ];
    conn.execute_batch(&sql, rows)?;
    let cursor = conn.query(
        &format!(
            "select t.id, t.point.x, t.point.label,
                (select listagg(column_value, ',') from table(t.numbers)),
                case when t.numbers is null then 'null' end
             from {table_name} t order by t.id"
        ),
        &[],
    )?;
    let mut rows = Vec::new();
    for row in cursor {
        let row = row?;
        rows.push(format!(
            "{}|{:?}|{:?}|{:?}|{:?}",
            row.get::<i32>(0)?,
            row.get::<Option<i32>>(1)?,
            row.get::<Option<String>>(2)?,
            row.get::<Option<String>>(3)?,
            row.get::<Option<String>>(4)?,
        ));
    }
    assert_eq!(
        rows,
        [
            "1|Some(1)|Some(\"one\")|Some(\"1,2\")|None",
            "2|None|None|None|Some(\"null\")",
            "3|Some(3)|Some(\"three\")|None|None",
        ]
    );

    // every row of a batch binds the same object type in a given position
    let other = oracledb::DbObject::new(&other_type)?;
    let rows: &[&[&dyn oracledb::ToDbValue]] =
        &[&[&4, &point1, &numbers1], &[&5, &other, &numbers1]];
    let error = conn.execute_batch(&sql, rows).unwrap_err();
    assert!(matches!(error.kind(), oracledb::ErrorKind::DifferentTypes(..)));
    Ok(())
}

#[rstest]
/// Re-executes a cached statement with objects of different types in one bind
/// position, and many IN OUT binds of a collection of objects in a row.
fn test_3724(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let point_type_name = "TEST_3724_POINT";
    let _point_type_guard = common::create_type(
        &conn,
        point_type_name,
        "object (x number, label varchar2(10))",
    )?;
    let points_type_name = "TEST_3724_POINTS";
    let _points_type_guard = common::create_type(
        &conn,
        points_type_name,
        &format!("table of {point_type_name}"),
    )?;
    let point_type = conn.get_type(point_type_name)?;
    let points_type = conn.get_type(points_type_name)?;
    let mut point = oracledb::DbObject::new(&point_type)?;
    point.set("X", 1)?;
    point.set("LABEL", "first")?;
    let mut points = oracledb::DbCollection::new(&points_type)?;
    points.push(point)?;

    // one PL/SQL text, executed with objects of different types
    let type_name = "declare
                         a anydata := anydata.convertobject(:value);
                     begin
                         :type_name := a.gettypename();
                     end;";
    let type_name_of = |value: &dyn oracledb::ToDbValue| {
        conn.execute_named(
            type_name,
            &[("value", value), ("type_name", &oracledb::DB_TYPE_VARCHAR)],
        )?
        .out_bind_data()
        .get::<String>("type_name")
    };
    let other_type_name = "TEST_3724_OTHER";
    let _other_type_guard =
        common::create_type(&conn, other_type_name, "object (y number)")?;
    let other_type = conn.get_type(other_type_name)?;
    for object_type in [&point_type, &other_type, &point_type] {
        assert_eq!(
            type_name_of(&oracledb::DbObject::new(object_type)?)?,
            object_type.fqn()
        );
    }

    // 200 IN OUT binds of a collection of objects
    let append = format!(
        r#"
        declare
            v {points_type_name} := :points;
        begin
            v.extend;
            v(v.last) := {point_type_name}(v.count, 'p' || v.count);
            :points := v;
        end;"#
    );
    for _ in 0..200 {
        let mut result =
            conn.execute_named(&append, &[("points", &points)])?;
        points = result.out_bind_data().take("points")?;
    }
    assert_eq!(points.len(), 201);
    let points: Vec<oracledb::DbObject> = points.try_into()?;
    assert_eq!(points[200].get::<String>("LABEL")?, "p201");
    Ok(())
}
