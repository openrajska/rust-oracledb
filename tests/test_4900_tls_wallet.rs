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
// test_4900_tls_wallet()
//
// These tests need TCPS listeners whose server certificate is signed by a
// private CA: RSO_TEST_TLS_CONNECT_STRING (one-way TLS) and
// RSO_TEST_MTLS_CONNECT_STRING (the client must present a certificate), and
// RSO_TEST_TLS_WALLETS, a directory holding the client wallets ca/ewallet.pem
// (the CA only) and mtls/ewallet.pem (the client's certificate and key, and
// the CA). They are skipped when the variables are not set.
//-----------------------------------------------------------------------------

use std::path::Path;

mod common;

/// Returns the connect string from the given variable and the path of the
/// named client wallet, or None if the test must be skipped.
fn tls_target(variable: &str, wallet: &str) -> Option<(String, String)> {
    let (Ok(connect_string), Ok(wallets)) =
        (std::env::var(variable), std::env::var("RSO_TEST_TLS_WALLETS"))
    else {
        common::skip_test(&format!(
            "requires {variable} and RSO_TEST_TLS_WALLETS"
        ));
        return None;
    };
    let wallet_location = Path::new(&wallets).join(wallet);
    Some((connect_string, wallet_location.to_string_lossy().into_owned()))
}

/// Connects with the given wallet and returns the network protocol of the
/// session.
fn network_protocol(
    connect_string: &str,
    wallet_location: &str,
) -> Result<String, oracledb::Error> {
    let test_config = oracledb::get_test_config();
    let config = oracledb::Config::default()
        .set_credentials(&test_config.user, &test_config.password)
        .set_wallet_location(wallet_location)
        .set_connect_string(connect_string)?;
    let conn = oracledb::connect(config)?;
    let row = conn.query_row(
        "select sys_context('userenv', 'network_protocol') from dual",
        &[],
    )?;
    row.get(0)
}

#[test]
/// Tests one-way TLS to a server certificate signed by a private CA, with a
/// wallet holding only that CA.
fn test_4900() -> Result<(), oracledb::Error> {
    let Some((connect_string, wallet)) =
        tls_target("RSO_TEST_TLS_CONNECT_STRING", "ca")
    else {
        return Ok(());
    };
    assert_eq!(network_protocol(&connect_string, &wallet)?, "tcps");
    Ok(())
}

#[test]
/// Tests mutual TLS to a server certificate signed by a private CA, with a
/// wallet holding the client's certificate and key, and that CA: the CA
/// must be trusted although the wallet holds a private key.
fn test_4901() -> Result<(), oracledb::Error> {
    let Some((connect_string, wallet)) =
        tls_target("RSO_TEST_MTLS_CONNECT_STRING", "mtls")
    else {
        return Ok(());
    };
    assert_eq!(network_protocol(&connect_string, &wallet)?, "tcps");
    Ok(())
}

#[test]
/// Tests that a server requiring a client certificate refuses a wallet
/// without a private key.
fn test_4902() {
    let Some((connect_string, wallet)) =
        tls_target("RSO_TEST_MTLS_CONNECT_STRING", "ca")
    else {
        return;
    };
    assert!(network_protocol(&connect_string, &wallet).is_err());
}
