                                                                         
                                                                       
                                                                              
                                                                               
#![cfg(feature = "rocket")]

use fig::{ApiError, catchers};
use rocket::http::Status;
use rocket::local::asynchronous::Client;
use rocket::serde::json::Json;
use rocket::{get, post, routes};
use std::time::Duration;

#[get("/test/not-found")]
fn route_not_found() -> ApiError {
    ApiError::not_found()
}

#[get("/test/not-found-custom")]
fn route_not_found_custom() -> ApiError {
    ApiError::NotFound {
        message: Some("file not found".to_string()),
    }
}

#[get("/test/bad-request")]
fn route_bad_request() -> ApiError {
    ApiError::BadRequest("missing field 'x'".to_string())
}

#[get("/test/forbidden")]
fn route_forbidden() -> ApiError {
    ApiError::forbidden()
}

#[get("/test/unauthorized")]
fn route_unauthorized() -> ApiError {
    ApiError::unauthorized()
}

#[get("/test/unauthorized-custom")]
fn route_unauthorized_custom() -> ApiError {
    ApiError::Unauthorized {
        message: Some("authentication required".to_string()),
    }
}

#[get("/test/conflict")]
fn route_conflict() -> ApiError {
    ApiError::Conflict("Operation blocked by a related record".to_string())
}

#[get("/test/throttled-no-window")]
fn route_throttled_no_window() -> ApiError {
    ApiError::TooManyRequests {
        message: "Too many login attempts".to_string(),
        retry_after: None,
    }
}

#[get("/test/throttled-with-window")]
fn route_throttled_with_window() -> ApiError {
    ApiError::TooManyRequests {
        message: "Too many pairing attempts".to_string(),
        retry_after: Some(Duration::from_secs(60)),
    }
}

#[get("/test/internal")]
fn route_internal() -> ApiError {
    ApiError::internal()
}

#[get("/test/range-not-satisfiable")]
fn route_range_not_satisfiable() -> ApiError {
    ApiError::RangeNotSatisfiable {
        message: None,
        content_range: Some("bytes */1024".to_string()),
    }
}

#[get("/test/gone")]
fn route_gone() -> ApiError {
    ApiError::Gone("The resource you're looking for has been revoked or removed".to_string())
}

#[get("/test/payload-too-large")]
fn route_payload_too_large() -> ApiError {
    ApiError::PayloadTooLarge("File too large (max 10 MiB)".to_string())
}

#[get("/test/unprocessable")]
fn route_unprocessable() -> ApiError {
    ApiError::Unprocessable("unprocessable entity".to_string())
}

#[rocket::async_test]
async fn not_found_serializes_correctly() {
    let rocket = rocket::build().mount("/", routes![route_not_found]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/not-found").dispatch().await;
    assert_eq!(resp.status(), Status::NotFound);
    assert_eq!(
        resp.headers().get_one("Content-Type"),
        Some("application/json; charset=utf-8"),
        "spec pins exact Content-Type bytes including charset"
    );
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"not found","status":404}"#);
}

#[rocket::async_test]
async fn bad_request_carries_message() {
    let rocket = rocket::build().mount("/", routes![route_bad_request]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/bad-request").dispatch().await;
    assert_eq!(resp.status(), Status::BadRequest);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"missing field 'x'","status":400}"#);
}

#[rocket::async_test]
async fn forbidden_serializes_correctly() {
    let rocket = rocket::build().mount("/", routes![route_forbidden]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/forbidden").dispatch().await;
    assert_eq!(resp.status(), Status::Forbidden);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"forbidden","status":403}"#);
}

#[rocket::async_test]
async fn unauthorized_serializes_correctly() {
    let rocket = rocket::build().mount("/", routes![route_unauthorized]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/unauthorized").dispatch().await;
    assert_eq!(resp.status(), Status::Unauthorized);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"unauthorized","status":401}"#);
}

#[rocket::async_test]
async fn conflict_carries_message() {
    let rocket = rocket::build().mount("/", routes![route_conflict]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/conflict").dispatch().await;
    assert_eq!(resp.status(), Status::Conflict);
    let body = resp.into_string().await.unwrap();
    assert_eq!(
        body,
        r#"{"error":"Operation blocked by a related record","status":409}"#
    );
}

#[rocket::async_test]
async fn throttled_without_window_emits_no_retry_after() {
    let rocket = rocket::build().mount("/", routes![route_throttled_no_window]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/throttled-no-window").dispatch().await;
    assert_eq!(resp.status(), Status::TooManyRequests);
    assert_eq!(resp.headers().get_one("Retry-After"), None);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"Too many login attempts","status":429}"#);
}

#[rocket::async_test]
async fn throttled_with_window_emits_retry_after() {
    let rocket = rocket::build().mount("/", routes![route_throttled_with_window]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/throttled-with-window").dispatch().await;
    assert_eq!(resp.status(), Status::TooManyRequests);
    assert_eq!(resp.headers().get_one("Retry-After"), Some("60"));
    let body = resp.into_string().await.unwrap();
    assert_eq!(
        body,
        r#"{"error":"Too many pairing attempts","status":429}"#
    );
}

#[rocket::async_test]
async fn internal_serializes_correctly() {
    let rocket = rocket::build().mount("/", routes![route_internal]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/internal").dispatch().await;
    assert_eq!(resp.status(), Status::InternalServerError);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"internal error","status":500}"#);
}

#[rocket::async_test]
async fn gone_carries_message() {
    let rocket = rocket::build().mount("/", routes![route_gone]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/gone").dispatch().await;
    assert_eq!(resp.status(), Status::Gone);
    let body = resp.into_string().await.unwrap();
    assert_eq!(
        body,
        r#"{"error":"The resource you're looking for has been revoked or removed","status":410}"#,
    );
}

#[rocket::async_test]
async fn payload_too_large_carries_message() {
    let rocket = rocket::build().mount("/", routes![route_payload_too_large]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/payload-too-large").dispatch().await;
    assert_eq!(resp.status(), Status::PayloadTooLarge);
    let body = resp.into_string().await.unwrap();
    assert_eq!(
        body,
        r#"{"error":"File too large (max 10 MiB)","status":413}"#
    );
}

#[rocket::async_test]
async fn unprocessable_carries_message() {
    let rocket = rocket::build().mount("/", routes![route_unprocessable]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/unprocessable").dispatch().await;
    assert_eq!(resp.status(), Status::UnprocessableEntity);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"unprocessable entity","status":422}"#);
}

#[rocket::async_test]
async fn not_found_carries_custom_message() {
    let rocket = rocket::build().mount("/", routes![route_not_found_custom]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/not-found-custom").dispatch().await;
    assert_eq!(resp.status(), Status::NotFound);
    let body = resp.into_string().await.unwrap();
                                                                 
                                                                        
                                                                    
                                                                   
                                                                          
    assert_eq!(body, r#"{"error":"file not found","status":404}"#);
}

#[rocket::async_test]
async fn unauthorized_carries_custom_message() {
    let rocket = rocket::build().mount("/", routes![route_unauthorized_custom]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/unauthorized-custom").dispatch().await;
    assert_eq!(resp.status(), Status::Unauthorized);
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"authentication required","status":401}"#);
}

#[rocket::async_test]
async fn range_not_satisfiable_emits_content_range_and_body() {
    let rocket = rocket::build().mount("/", routes![route_range_not_satisfiable]);
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/test/range-not-satisfiable").dispatch().await;
    assert_eq!(resp.status(), Status::RangeNotSatisfiable);
    assert_eq!(
        resp.headers().get_one("Content-Range"),
        Some("bytes */1024"),
        "RFC 9110 §14.4 SHOULD: 416 includes Content-Range with the unsatisfied-range indicator"
    );
    let body = resp.into_string().await.unwrap();
                                                   
    assert_eq!(body, r#"{"error":"range not satisfiable","status":416}"#);
}

                                                                                                                      
  
                                                                      
                                                                            
                                                                      
                                                                           
                                  
  
                                                                           
                                                                          
                                                                    
                                                                    
                                                                            

#[derive(serde::Deserialize)]
struct ExpectsField {
    #[allow(dead_code)]
    name: String,
}

#[post("/needs-json", data = "<_body>")]
fn route_needs_json(_body: Json<ExpectsField>) -> &'static str {
    "ok"
}

#[rocket::async_test]
async fn catcher_404_emits_json_contract() {
    let rocket = rocket::build()
        .mount("/", routes![route_not_found])
        .register("/", catchers());
    let client = Client::tracked(rocket).await.unwrap();
    let resp = client.get("/no-such-route").dispatch().await;
    assert_eq!(resp.status(), Status::NotFound);
    assert_eq!(
        resp.headers().get_one("Content-Type"),
        Some("application/json; charset=utf-8"),
        "spec pins exact Content-Type bytes including charset"
    );
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"not found","status":404}"#);
}

#[rocket::async_test]
async fn catcher_400_emits_json_contract_for_malformed_json() {
    let rocket = rocket::build()
        .mount("/", routes![route_needs_json])
        .register("/", catchers());
    let client = Client::tracked(rocket).await.unwrap();
                                                                              
    let resp = client
        .post("/needs-json")
        .header(rocket::http::ContentType::JSON)
        .body("not-json")
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::BadRequest);
    assert_eq!(
        resp.headers().get_one("Content-Type"),
        Some("application/json; charset=utf-8"),
        "spec pins exact Content-Type bytes including charset"
    );
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"bad request","status":400}"#);
}

#[rocket::async_test]
async fn catcher_422_emits_json_contract_for_missing_field() {
    let rocket = rocket::build()
        .mount("/", routes![route_needs_json])
        .register("/", catchers());
    let client = Client::tracked(rocket).await.unwrap();
                                                                         
                                                                              
    let resp = client
        .post("/needs-json")
        .header(rocket::http::ContentType::JSON)
        .body(r#"{"other_field": 1}"#)
        .dispatch()
        .await;
    assert_eq!(resp.status(), Status::UnprocessableEntity);
    assert_eq!(
        resp.headers().get_one("Content-Type"),
        Some("application/json; charset=utf-8"),
        "spec pins exact Content-Type bytes including charset"
    );
    let body = resp.into_string().await.unwrap();
    assert_eq!(body, r#"{"error":"unprocessable entity","status":422}"#);
}
