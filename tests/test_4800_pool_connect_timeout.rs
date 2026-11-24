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
// test_4800_pool_connect_timeout()
//-----------------------------------------------------------------------------

use std::time::{Duration, Instant};

#[test]
/// Tests that the transport connect timeout of a pool configuration defaults
/// to none and can be set and cleared.
fn test_4800() {
    let config = oracledb::PoolConfig::default();
    assert_eq!(config.transport_connect_timeout(), None);
    let config =
        config.set_transport_connect_timeout(Some(Duration::from_secs(3)));
    assert_eq!(
        config.transport_connect_timeout(),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        config.connection_config().transport_connect_timeout(),
        Some(Duration::from_secs(3))
    );
    let config = config.set_transport_connect_timeout(None);
    assert_eq!(config.transport_connect_timeout(), None);
}

#[test]
/// Tests that acquiring a connection from a pool whose host does not answer
/// fails once the transport connect timeout has passed, instead of waiting
/// for the operating system's own connect timeout (minutes).
fn test_4801() {
    let test_config = oracledb::get_test_config();
    // 192.0.2.0/24 is reserved for documentation (RFC 5737): nothing answers
    // there.
    let config = oracledb::PoolConfig::default()
        .set_credentials(&test_config.user, &test_config.password)
        .set_transport_connect_timeout(Some(Duration::from_secs(1)))
        .set_connect_string("192.0.2.1:1521/FREEPDB1")
        .unwrap();
    let pool = oracledb::create_pool(config).unwrap();
    let started = Instant::now();
    let error = pool.acquire().unwrap_err();
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(10),
        "acquire() took {elapsed:?}: {error}"
    );
}
