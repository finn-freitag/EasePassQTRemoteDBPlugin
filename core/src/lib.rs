//! Logic of the plugin, independent of the WebAssembly interface so it can be tested
//! natively: the plugin offers every configured address as a READ-ONLY database.
//!
//! Nothing is fetched when the databases are listed (Ease Pass lists them when it starts);
//! the file is downloaded only when the user opens the database.

use serde::{Deserialize, Serialize};

pub const MAX_URLS: usize = 16;
pub const MAX_URL_LENGTH: usize = 2048;
/// Ease Pass can move about 2.5 MiB of database through a plugin in one call (every byte
/// crosses the plugin boundary twice); 2 MiB is the limit this plugin promises.
pub const MAX_DATABASE_BYTES: usize = 2 * 1024 * 1024;
const KEY_CONFIG: &str = "config";

pub const CONFIG_SCHEMA: &str = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"address":{"type":"string","title":"Address of the database file","description":"For example https://example.org/files/family.epdb. Several addresses can be separated by a space. The databases are opened read-only.","default":"","maxLength":4096,"propertyOrder":1},"offline_copy":{"type":"boolean","title":"Keep a copy for when the address cannot be reached","default":false,"propertyOrder":2}}}"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    InvalidArgument,
    NotFound,
    NotAllowed,
    Unavailable,
    TooLarge,
    Network,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: Code,
    pub message: String,
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Error { code, message: message.into() }
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// What the plugin needs from Ease Pass.
pub trait Host {
    fn get_string(&self, key: &str) -> Result<Option<String>, Error>;
    fn set_string(&self, key: &str, value: &str) -> Result<(), Error>;
    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, Error>;
    fn write_file(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    fn delete_file(&self, name: &str) -> Result<(), Error>;
    fn list_files(&self) -> Result<Vec<String>, Error>;
    fn http_get(&self, url: &str) -> Result<HttpResponse, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub address: String,
    pub offline_copy: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { address: String::new(), offline_copy: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Database {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
}

/// The parts of an http(s) address this plugin cares about.
struct ParsedUrl<'a> {
    scheme: &'a str,
    authority: &'a str,
    path: &'a str,
}

fn parse_url(url: &str) -> Result<ParsedUrl<'_>, Error> {
    let invalid = |why: &str| Error::new(Code::InvalidArgument, format!("\"{}\" {}", shorten(url), why));
    if url.len() > MAX_URL_LENGTH {
        return Err(invalid("is too long"));
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(invalid("contains characters that are not allowed in an address"));
    }
    let (scheme, rest) = url.split_once("://").ok_or_else(|| invalid("is not an address (https://...)"))?;
    let scheme_ok = scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http");
    if !scheme_ok {
        return Err(invalid("must start with https:// or http://"));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // A user name or password in the address would end up in lists and logs.
    if authority.is_empty() || authority.contains('@') {
        return Err(invalid("must name a server (without user name or password)"));
    }
    let tail = &rest[end..];
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    Ok(ParsedUrl { scheme: if scheme.eq_ignore_ascii_case("https") { "https" } else { "http" }, authority, path: &tail[..path_end] })
}

fn shorten(text: &str) -> String {
    let mut short: String = text.chars().filter(|c| !c.is_control()).take(80).collect();
    if text.chars().count() > 80 {
        short.push_str("...");
    }
    short
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// FNV-1a: a short, stable id for an address (not a security feature).
fn id_of(url: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn database_of(url: &str) -> Result<Database, Error> {
    let parsed = parse_url(url)?;
    let file = parsed.path.rsplit('/').next().unwrap_or("");
    let decoded = percent_decode(file);
    let stem = match decoded.rfind('.') {
        Some(dot) if dot > 0 => &decoded[..dot],
        _ => decoded.as_str(),
    };
    let name: String = stem.chars().filter(|c| !c.is_control()).take(80).collect();
    let name = if name.trim().is_empty() { parsed.authority.to_string() } else { name };
    Ok(Database {
        id: id_of(url),
        name,
        // Without the query: it may carry an access token.
        description: format!("{}://{}{} (read-only)", parsed.scheme, parsed.authority, parsed.path),
        url: url.to_string(),
    })
}

/// The databases named by `config.address` (addresses separated by white space).
pub fn databases_of(config: &Config) -> Result<Vec<Database>, Error> {
    let mut databases: Vec<Database> = Vec::new();
    for url in config.address.split_whitespace() {
        let database = database_of(url)?;
        if databases.iter().any(|d| d.id == database.id) {
            continue; // the same address twice
        }
        databases.push(database);
        if databases.len() > MAX_URLS {
            return Err(Error::new(Code::InvalidArgument, format!("at most {MAX_URLS} addresses are possible")));
        }
    }
    Ok(databases)
}

pub fn load_config(host: &dyn Host) -> Result<Config, Error> {
    match host.get_string(KEY_CONFIG)? {
        None => Ok(Config::default()),
        // A stored configuration that cannot be read any more counts as "nothing configured".
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
    }
}

pub fn get_config(host: &dyn Host) -> Result<String, Error> {
    serde_json::to_string(&load_config(host)?).map_err(|e| Error::new(Code::Internal, e.to_string()))
}

pub fn set_config(host: &dyn Host, json: &str) -> Result<(), Error> {
    let config: Config = serde_json::from_str(json)
        .map_err(|_| Error::new(Code::InvalidArgument, "the settings do not have the expected form"))?;
    let databases = databases_of(&config)?;
    let stored = serde_json::to_string(&config).map_err(|e| Error::new(Code::Internal, e.to_string()))?;
    host.set_string(KEY_CONFIG, &stored)?;
    // Copies of databases that are not configured any more (or not wanted any more) go away.
    for file in host.list_files()? {
        let keep = config.offline_copy && databases.iter().any(|d| copy_name(&d.id) == file);
        if !keep {
            host.delete_file(&file)?;
        }
    }
    Ok(())
}

pub fn list_databases(host: &dyn Host) -> Result<Vec<Database>, Error> {
    databases_of(&load_config(host)?)
}

pub fn find_database(host: &dyn Host, id: &str) -> Result<Database, Error> {
    list_databases(host)?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| Error::new(Code::NotFound, "this database is not configured any more"))
}

fn copy_name(id: &str) -> String {
    format!("{id}.epdb")
}

/// Downloads the database. With `offline_copy` the last download is used when the address
/// cannot be reached.
pub fn read_database(host: &dyn Host, id: &str) -> Result<Vec<u8>, Error> {
    let database = find_database(host, id)?;
    let offline_copy = load_config(host)?.offline_copy;
    let downloaded = download(host, &database.url);
    match downloaded {
        Ok(bytes) => {
            if offline_copy {
                // Failing to keep the copy must not stop the user from opening the database.
                let _ = host.write_file(&copy_name(id), &bytes);
            }
            Ok(bytes)
        }
        Err(error) => {
            // The copy stands in for an address that does not answer or is broken. It does
            // not hide that access was withdrawn or that the file became too large.
            let stand_in = !matches!(error.code, Code::NotAllowed | Code::TooLarge);
            if offline_copy && stand_in {
                if let Ok(Some(copy)) = host.read_file(&copy_name(id)) {
                    return Ok(copy);
                }
            }
            Err(error)
        }
    }
}

fn download(host: &dyn Host, url: &str) -> Result<Vec<u8>, Error> {
    let response = host.http_get(url)?;
    match response.status {
        200 => {}
        404 | 410 => return Err(Error::new(Code::NotFound, "there is no file at this address (the server answered \"not found\")")),
        401 | 403 => return Err(Error::new(Code::NotAllowed, "the server does not allow access to this address")),
        status => return Err(Error::new(Code::Unavailable, format!("the server answered with status {status}"))),
    }
    if response.body.is_empty() {
        return Err(Error::new(Code::Unavailable, "the server sent an empty file"));
    }
    if response.body.len() > MAX_DATABASE_BYTES {
        return Err(Error::new(Code::TooLarge, "the file is larger than 2 MiB"));
    }
    Ok(response.body)
}

/// Databases of this plugin can never be written.
pub fn write_database() -> Error {
    Error::new(Code::NotAllowed, "databases opened from a URL are read-only")
}
