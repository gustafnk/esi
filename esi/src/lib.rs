use quick_xml::{
    events::{BytesStart, BytesText, Event},
    Reader, Writer,
};
use std::{collections::HashMap, io::BufRead};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error("xml parsing error: {0}")]
    XMLError(#[from] quick_xml::Error),
    #[error("tag `{0}` is missing required parameter `{1}`")]
    MissingRequiredParameter(String, String),
    #[error("unexpected `{0}` closing tag")]
    UnexpectedClosingTag(String),
    #[error("duplicate attribute detected: {0}")]
    DuplicateTagAttribute(String),
    #[error("unknown error")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, ExecutionError>;

/// A request initiated by the ESI executor.
#[derive(Debug)]
pub struct Request {
    pub url: String,
}

impl Request {
    fn from_url(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

/// A response from the local `ExecutionContext` implementation.
/// Usually the result of a `Request`.
#[derive(Debug)]
pub struct Response {
    pub body: Vec<u8>,
    pub status_code: u16,
}

/// Handles requests to backends as part of the ESI execution process.
/// Implemented by `esi_fastly::FastlyRequestHandler`.
pub trait ExecutionContext {
    /// Sends a request to the given URL and returns either an error or the response body.
    /// Returns response body.
    fn send_request(&self, req: Request) -> Result<Response>;
}

/// Representation of an ESI tag from a source response.
#[derive(Debug)]
pub struct Tag {
    name: Vec<u8>,                         // "include"
    content: Option<String>,               // "hello world"
    parameters: HashMap<Vec<u8>, Vec<u8>>, // src = "/a.html"
}

impl Tag {
    fn get_param(&self, key: &str) -> Option<String> {
        self.parameters.get(key.as_bytes()).map(|value| String::from_utf8(value.to_owned()).unwrap())
    }
}

pub struct TagEntry<'a> {
    event: Option<Event<'a>>,
    esi_tag: Option<Tag>,
}

// This could be much cleaner but I'm not good enough at Rust for that
fn parse_attributes(bytes: BytesStart) -> Result<HashMap<Vec<u8>, Vec<u8>>> {
    let mut map: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    for entry in bytes.attributes().flatten() {
        if map.insert(entry.key.to_vec(), entry.value.to_vec()).is_some() {
            return Err(ExecutionError::DuplicateTagAttribute(String::from_utf8(entry.key.to_vec()).unwrap()));
        }
    }

    Ok(map)
}

fn parse_tag_entries<'a>(body: impl BufRead) -> Result<Vec<TagEntry<'a>>> {
    let mut reader = Reader::from_reader(body);
    let mut buf = Vec::new();

    let mut events: Vec<TagEntry> = Vec::new();
    let mut remove = false;

    // Parse tags and build events vec
    loop {
        buf.clear();
        match reader.read_event(&mut buf) {
            // Handle <esi:remove> tags
            Ok(Event::Start(elem)) if elem.starts_with(b"esi:remove") => {
                remove = true;
            }
            Ok(Event::End(elem)) if elem.starts_with(b"esi:remove") => {
                if !remove {
                    return Err(ExecutionError::UnexpectedClosingTag(String::from_utf8(elem.to_vec()).unwrap()));
                }

                remove = false;
            }
            _ if remove => continue,

            // Parse empty ESI tags
            Ok(Event::Empty(elem)) if elem.name().starts_with(b"esi:") => {
                events.push(TagEntry {
                    event: None,
                    esi_tag: Some(Tag {
                        name: elem.name().to_vec(),
                        parameters: parse_attributes(elem)?,
                        content: None,
                    }),
                });
            }

            Ok(Event::Eof) => break,
            Ok(e) => events.push(TagEntry {
                event: Some(e.into_owned()),
                esi_tag: None,
            }),
            _ => {}
        }
    }

    Ok(events)
}

// Executes all entries with an ESI tag, and returns a map of those entries with the entry's index as key and content as value.
fn execute_tag_entries(
    entries: &[TagEntry],
    client: &impl ExecutionContext,
) -> Result<HashMap<usize, Vec<u8>>> {
    let mut map = HashMap::new();

    for (index, entry) in entries.iter().enumerate() {
        match &entry.esi_tag {
            Some(tag) => {
                if tag.name == b"esi:include" {
                    let src = match tag.get_param("src") {
                        Some(src) => src,
                        None => {
                            return Err(ExecutionError::MissingRequiredParameter(
                                String::from_utf8(tag.name.to_vec()).unwrap(),
                                "src".to_string(),
                            ));
                        }
                    };

                    let alt = tag.get_param("alt");

                    match send_request(&src, alt, client) {
                        Ok(resp) => {
                            map.insert(index, resp.body).unwrap();
                        },
                        Err(err) => match tag.get_param("onerror") {
                            Some(onerror) => {
                                if onerror == "continue" {
                                    println!("Failed to fetch {} but continued", src);
                                    map.insert(index, vec![]).unwrap();
                                } else {
                                    return Err(err);
                                }
                            }
                            _ => return Err(err),
                        },
                    }
                }
            }
            None => {}
        }
    }

    Ok(map)
}

/// Processes a given ESI response body and returns the transformed body after all ESI instructions
/// have been executed.
pub fn transform_esi_string(
    body: impl BufRead,
    client: &impl ExecutionContext,
) -> Result<Vec<u8>> {
    // Parse tags
    let events = parse_tag_entries(body)?;

    // Execute tags
    let results = execute_tag_entries(&events, client)?;

    // Build output XML
    let mut writer = Writer::new(Vec::new());

    for (index, entry) in events.iter().enumerate() {
        match &entry.esi_tag {
            Some(_tag) => if let Some(content) = results.get(&index) {
                writer
                    .write_event(Event::Text(BytesText::from_escaped(content)))
                    .unwrap();
            },
            _ => match &entry.event {
                Some(event) => {
                    writer.write_event(event).unwrap();
                }
                None => {}
            },
        }
    }

    println!("esi processing done.");

    Ok(writer.into_inner())
}

/// Sends a request to the given `src`, optionally falling back to the `alt` if the first request is not successful.
fn send_request(
    src: &str,
    alt: Option<String>,
    client: &impl ExecutionContext,
) -> Result<Response> {
    match client.send_request(Request::from_url(src)) {
        Ok(resp) => Ok(resp),
        Err(err) => match alt {
            Some(alt) => match client.send_request(Request::from_url(&alt)) {
                Ok(resp) => Ok(resp),
                Err(_) => Err(err),
            },
            None => Err(err),
        },
    }
}
