use std::str::FromStr;

use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue, errors::InvalidMetadataValue},
};

pub fn token_to_metadata_value(
    token: impl AsRef<str>,
) -> Result<MetadataValue<Ascii>, InvalidMetadataValue> {
    MetadataValue::from_str(&format!("Bearer {}", token.as_ref()))
}

pub fn extract_user_token(token: &MetadataValue<Ascii>) -> Result<String, String> {
    match token.to_str() {
        Ok(token) if token.starts_with("Bearer ") => Ok(token[7..].to_string()),
        Ok(_) => Err("parsed token must start with 'Bearer '".to_owned()),
        _ => Err("parsed token contains invisible ASCII characters".to_owned()),
    }
}

pub fn check_token(
    auth_token: Option<MetadataValue<Ascii>>,
) -> impl Fn(Request<()>) -> Result<Request<()>, Status> + Clone {
    move |req: Request<()>| match &auth_token {
        Some(token) => match req.metadata().get(AUTHORIZATION) {
            Some(t) if t == token => Ok(req),
            Some(_) => Err(Status::unauthenticated("Invalid auth token")),
            _ => Err(Status::unauthenticated("Missing auth token")),
        },
        None => Ok(req),
    }
}

pub const AUTHORIZATION: &str = "authorization";

#[cfg(test)]
mod tests {
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

    #[test]
    fn check_token_intercepts_request_and_validates_token_when_token_is_supplied() {
        let token = "my-token";
        let check_fn = check_token(Some(MetadataValue::from_static(token)));

        {
            // No token in request
            let result = check_fn(Request::new(()));
            assert!(result.is_err());
            let error = result.unwrap_err();
            assert_eq!(error.code(), Code::Unauthenticated);
            assert!(error.message().contains("Missing auth token"));
        }
        {
            // Invalid token in request
            let mut req = Request::new(());
            req.metadata_mut()
                .insert(AUTHORIZATION, MetadataValue::from_static("invalid-token"));
            let result = check_fn(req);
            assert!(result.is_err());
            let error = result.unwrap_err();
            assert_eq!(error.code(), Code::Unauthenticated);
            assert!(error.message().contains("Invalid auth token"));
        }
        {
            // Valid token in request
            let mut req = Request::new(());
            req.metadata_mut()
                .insert(AUTHORIZATION, MetadataValue::from_static(token));
            let result = check_fn(req);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn check_token_forwards_request_when_no_token_is_supplied() {
        let check_fn = check_token(None);

        {
            // No token in request
            let result = check_fn(Request::new(()));
            assert!(result.is_ok());
        }
        {
            // Some token in request
            let mut req = Request::new(());
            req.metadata_mut()
                .insert(AUTHORIZATION, MetadataValue::from_static("my-token"));
            let result = check_fn(req);
            assert!(result.is_ok());
        }
    }
}
