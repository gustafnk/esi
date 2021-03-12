# esi

A barebones Rust implementation of Edge Side Includes. Compatible with Fastly Compute@Edge via the [`esi_fastly`](https://docs.rs/esi_fastly) crate.

Goal is to fully implement the [ESI Language Specification 1.0](https://www.w3.org/TR/esi-lang/).

## Supported Tags

- `<esi:include>` (no support for alt or onerror yet)
- `<esi:comment>`
- `<esi:remove>`

## Usage

### Compute@Edge

The [`esi_fastly`](https://docs.rs/esi_fastly) crate provides an implementation of a `RequestHandler` that will automatically pass requests to backends matching the request hostname. Make sure create a backend for every host that your application will serve.

#### Cargo.toml

```toml
[dependencies]
esi_fastly = "^0.1"
```

#### src/main.rs

```rust
use fastly::{Error, Request, Response};
use esi_fastly::process_esi;

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    // Send request to backend.
    let beresp = req.send("backend")?;

    // Process and execute ESI tags within the response body.
    // Make sure you have backends defined for any included hosts.
    // Their names should match the hostname, e.g. "developer.fastly.com"
    let result = process_esi(req, beresp)?;

    // Return the updated response to the client.
    Ok(result)
}
```


### Standalone Rust

To use the [`esi`](https://docs.rs/esi) crate without a third-party `RequestHandler`, you will have to implement one yourself. The example below shows a basic request handler that uses the `reqwest` crate.

#### Cargo.toml

```toml
[dependencies]
esi = "^0.1"
```

#### src/main.rs

```rust
pub struct ReqwestHandler;

impl esi::RequestHandler for ReqwestHandler {
    fn send_request(&self, url: &str) -> Result<String, esi::Error> {
        match reqwest::blocking::get(url) {
            Ok(resp) => Ok(resp.text().unwrap()),
            Err(err) => Err(esi::Error::from_message(&format!("{:?}", err)))
        }
    }
}
```


```rust
use esi::transform_esi_string;

let req_handler = ReqwestHandler {};

match transform_esi_string(response_body, &req_handler) {
    Ok(body) => response.set_body(body),
    Err(err) => panic!()
}
```

## License

The source and documentation for this project are released under the [MIT License](LICENSE).
