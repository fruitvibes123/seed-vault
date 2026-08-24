use rocket::request::Request;
use rocket::{Catcher, catch};

use crate::responder::ApiError;

#[catch(400)]
fn bad_request_catcher(_req: &Request) -> ApiError {
    ApiError::BadRequest("bad request".to_string())
}

#[catch(404)]
fn not_found_catcher(_req: &Request) -> ApiError {
    ApiError::not_found()
}

#[catch(422)]
fn unprocessable_catcher(_req: &Request) -> ApiError {
    ApiError::Unprocessable("unprocessable entity".to_string())
}

#[catch(500)]
fn internal_catcher(_req: &Request) -> ApiError {
    ApiError::internal()
}

/// Returns the catcher set to register for a JSON-API prefix. Mount with
/// `rocket.register("/api/v1", fig::catchers())`.
///
/// Covers framework-level errors at the registered prefix:
/// - 400 on malformed JSON bodies (Rocket's Json data guard parse failure)
/// - 404 on unmatched paths
/// - 422 on request bodies that parse but fail semantic validation
///   (e.g., missing required JSON field)
/// - 500 on unhandled internal errors
pub fn catchers() -> Vec<Catcher> {
    rocket::catchers![
        bad_request_catcher,
        not_found_catcher,
        unprocessable_catcher,
        internal_catcher,
    ]
}
