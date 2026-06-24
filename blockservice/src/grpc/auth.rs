// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use std::str::FromStr;

use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue, errors::InvalidMetadataValue},
};

/// Converts a token string into a gRPC metadata value with `Bearer ` prefix.
pub fn token_to_metadata_value(
    token: impl AsRef<str>,
) -> Result<MetadataValue<Ascii>, InvalidMetadataValue> {
    MetadataValue::from_str(&format!("Bearer {}", token.as_ref()))
}

/// Extracts the user token from a gRPC metadata value, removing the `Bearer ` prefix.
pub fn extract_user_token(token: &MetadataValue<Ascii>) -> Result<String, String> {
    match token.to_str() {
        Ok(token) if token.starts_with("Bearer ") => Ok(token[7..].to_string()),
        Ok(_) => Err("parsed token must start with 'Bearer '".to_owned()),
        _ => Err("parsed token contains invisible ASCII characters".to_owned()),
    }
}

/// Returns a closure which implements the authentication check. If a token is specified, it
/// extracts the authentication header from a gRPC request and checks it against the provided token.
pub fn check_token(
    auth_token: Option<MetadataValue<Ascii>>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| match &auth_token {
        Some(token) => match req.metadata().get(AUTHORIZATION_HEADER_NAME) {
            Some(t) if t == token => Ok(req),
            Some(_) => Err(Status::unauthenticated("Invalid auth token")),
            _ => Err(Status::unauthenticated("Missing auth token")),
        },
        None => Ok(req),
    }
}

/// The name of the HTTP authorization header.
pub const AUTHORIZATION_HEADER_NAME: &str = "authorization";

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tonic::Code;

    use super::*;

    #[test]
    fn token_to_metadata_value_converts_valid_token() {
        let token = "my-token";
        let result = token_to_metadata_value(token);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            MetadataValue::from_str(&format!("Bearer {token}")).unwrap()
        );
    }

    #[test]
    fn token_to_metadata_value_fails_with_invalid_token() {
        let result = token_to_metadata_value("\u{0}");
        assert!(result.is_err());
    }

    #[test]
    fn extract_user_token_succeeds_on_valid_token() {
        let token = "my-token";
        let result = extract_user_token(&token_to_metadata_value(token).unwrap());
        assert_eq!(result, Ok(token.to_string()));
    }

    #[test]
    fn extract_user_token_fails_if_token_is_not_a_bearer_token() {
        let token = "my-token";
        let result = extract_user_token(&MetadataValue::from_str(token).unwrap());
        assert_eq!(
            result,
            Err("parsed token must start with 'Bearer '".to_owned())
        );
    }

    #[rstest]
    #[case(None, Some("Missing auth token"))]
    #[case(Some("invalid-token"), Some("Invalid auth token"))]
    #[case(Some("Bearer my-token"), None)]
    fn check_token_intercepts_request_and_validates_token_when_token_is_supplied(
        #[case] header_value: Option<&str>,
        #[case] expected_error_msg: Option<&str>,
    ) {
        let token = token_to_metadata_value("my-token").unwrap();
        let check_fn = check_token(Some(token));
        let mut req = Request::new(());

        if let Some(header_value) = header_value {
            req.metadata_mut().insert(
                AUTHORIZATION_HEADER_NAME,
                MetadataValue::from_str(header_value).unwrap(),
            );
        }

        let result = check_fn(req);

        if let Some(expected_error_msg) = expected_error_msg {
            assert!(result.is_err());
            let error = result.unwrap_err();
            assert_eq!(error.code(), Code::Unauthenticated);
            assert!(error.message().contains(expected_error_msg));
        } else {
            assert!(result.is_ok());
        }
    }

    #[rstest]
    #[case::no_token_in_request(None)]
    #[case::token_in_request(Some("my-token"))]
    fn check_token_forwards_request_when_no_token_is_supplied(
        #[case] header_value: Option<&'static str>,
    ) {
        let check_fn = check_token(None);
        let mut req = Request::new(());
        if let Some(header_value) = header_value {
            req.metadata_mut().insert(
                AUTHORIZATION_HEADER_NAME,
                MetadataValue::from_static(header_value),
            );
        }
        let result = check_fn(req);
        assert!(result.is_ok());
    }
}
