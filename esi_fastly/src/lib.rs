use std::str::FromStr;

use esi::{ExecutionContext, transform_esi_string, ExecutionError};
use fastly::{Request, Response, http::{Url, header}};

/// A request handler that, given a `fastly::Request`, will route requests to a backend matching
/// the hostname of the request URL.
pub struct FastlyRequestHandler {
    original_req: Request
}

impl FastlyRequestHandler {
    fn from_request(req: Request) -> FastlyRequestHandler {
        FastlyRequestHandler {
            original_req: req
        }
    }
}

impl ExecutionContext for FastlyRequestHandler {
    fn send_request(&self, req: esi::Request) -> Result<esi::Response, ExecutionError> {
        println!("Sending request: {:?}", req);

        let mut bereq = self.original_req.clone_without_body().with_url(&req.url);

        // assume that backend name == host
        let parsed_url = Url::from_str(&req.url).unwrap();
        let backend = parsed_url.host_str().unwrap();
        bereq.set_header(header::HOST, backend);

        let mut beresp = match bereq.send(backend) {
            Ok(resp) => resp,
            Err(_) => panic!("Error sending ESI include request to backend {}", backend)
        };

        println!("Received response: {}", beresp.get_status().as_u16());

        if beresp.get_status().as_u16() < 200 || beresp.get_status().as_u16() > 299 {
            // TODO: handle bad status codes
            return Err(ExecutionError::Unknown);
        }

        let resp = esi::Response {
            body: beresp.take_body_bytes(),
            status_code: beresp.get_status().as_u16()
        };

        println!("Response passed to esi processor");
        Ok(resp)
    }
}

/// Processes the body of a `fastly::Response` and returns an updated Response after executing
/// all found ESI instructions.
///
/// # Examples
/// ```no_run
/// use fastly::{Error, Request, Response};
/// use esi_fastly::process_esi;
///
/// #[fastly::main]
/// fn main(req: Request) -> Result<Response, Error> {
///     let beresp = req.send("backend")?;
///     process_esi(req, beresp);
/// }
/// ```
pub fn process_esi(req: Request, mut response: Response) -> Result<Response, fastly::Error> {
    let req_handler = FastlyRequestHandler::from_request(req);

    match transform_esi_string(response.take_body(), &req_handler) {
        Ok(body) => response.set_body(body),
        Err(err) => return Err(fastly::Error::from(err)),
    }

    Ok(response)
}
