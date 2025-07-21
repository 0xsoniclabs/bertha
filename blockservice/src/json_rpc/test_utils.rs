use serde::{Deserialize, Serialize};
use wiremock::{Mock, Request, ResponseTemplate, matchers};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    id: usize,
    method: String,
    params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcRequestWithoutId {
    jsonrpc: String,
    method: String,
    params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RpcResponse {
    jsonrpc: String,
    id: usize,
    result: serde_json::Value,
}

pub fn build_mock_server_request_handler_for_single_request<T>(
    method: &str,
    id: usize,
    params: Vec<serde_json::Value>,
    result: T,
) -> Mock
where
    T: Send + Sync + Serialize + 'static,
{
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/"))
        .and(matchers::body_json(RpcRequest {
            jsonrpc: "2.0".to_owned(),
            id,
            method: method.to_owned(),
            params,
        }))
        .respond_with(move |req: &Request| {
            let req = serde_json::from_slice::<RpcRequest>(&req.body).unwrap();
            ResponseTemplate::new(200).set_body_json(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: serde_json::to_value(&result).unwrap(),
            })
        })
        .expect(1) // expect the request to be made once
}

pub fn build_mock_server_request_handler_for_infinitely_many_requests<T>(
    method: &str,
    params: Vec<serde_json::Value>,
    result: T,
) -> Mock
where
    T: Send + Sync + Serialize + 'static,
{
    Mock::given(matchers::method("POST"))
        .and(matchers::path("/"))
        .and(matchers::body_partial_json(RpcRequestWithoutId {
            jsonrpc: "2.0".to_owned(),
            method: method.to_owned(),
            params,
        }))
        .respond_with(move |req: &Request| {
            let req = serde_json::from_slice::<RpcRequest>(&req.body).unwrap();
            ResponseTemplate::new(200).set_body_json(RpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: serde_json::to_value(&result).unwrap(),
            })
        })
}
