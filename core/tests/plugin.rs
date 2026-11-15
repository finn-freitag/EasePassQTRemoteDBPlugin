use easepass_url_database_core::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Storage and network of Ease Pass, in memory.
#[derive(Default)]
struct FakeHost {
    strings: RefCell<BTreeMap<String, String>>,
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// url -> (status, body); a missing url is a network error
    web: RefCell<BTreeMap<String, (u16, Vec<u8>)>>,
    requests: RefCell<Vec<String>>,
    fail_writes: RefCell<bool>,
}

impl Host for FakeHost {
    fn get_string(&self, key: &str) -> Result<Option<String>, Error> {
        Ok(self.strings.borrow().get(key).cloned())
    }
    fn set_string(&self, key: &str, value: &str) -> Result<(), Error> {
        self.strings.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.files.borrow().get(name).cloned())
    }
    fn write_file(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        if *self.fail_writes.borrow() {
            return Err(Error::new(Code::Internal, "disk full"));
        }
        self.files.borrow_mut().insert(name.into(), data.to_vec());
        Ok(())
    }
    fn delete_file(&self, name: &str) -> Result<(), Error> {
        self.files.borrow_mut().remove(name);
        Ok(())
    }
    fn list_files(&self) -> Result<Vec<String>, Error> {
        Ok(self.files.borrow().keys().cloned().collect())
    }
    fn http_get(&self, url: &str) -> Result<HttpResponse, Error> {
        self.requests.borrow_mut().push(url.into());
        match self.web.borrow().get(url) {
            Some((status, body)) => Ok(HttpResponse { status: *status, body: body.clone() }),
            None => Err(Error::new(Code::Network, "no connection")),
        }
    }
}

fn configure(host: &FakeHost, address: &str, offline_copy: bool) -> Result<(), Error> {
    set_config(host, &serde_json::json!({"address": address, "offline_copy": offline_copy}).to_string())
}

const URL: &str = "https://example.org/files/Family%20vault.epdb?token=secret";

#[test]
fn nothing_configured_means_no_database() {
    let host = FakeHost::default();
    assert!(list_databases(&host).unwrap().is_empty());
    assert_eq!(get_config(&host).unwrap(), r#"{"address":"","offline_copy":false}"#);
    // The stored configuration is unreadable (older version, damaged): same as none.
    host.set_string("config", "not json").unwrap();
    assert!(list_databases(&host).unwrap().is_empty());
}

#[test]
fn the_schema_is_a_flat_object_the_form_can_show() {
    let schema: serde_json::Value = serde_json::from_str(CONFIG_SCHEMA).unwrap();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["address"]["type"], "string");
    assert_eq!(schema["properties"]["offline_copy"]["type"], "boolean");
    // The default configuration matches the schema's defaults.
    let default: serde_json::Value = serde_json::from_str(&get_config(&FakeHost::default()).unwrap()).unwrap();
    for (key, property) in schema["properties"].as_object().unwrap() {
        assert_eq!(default[key], property["default"], "{key}");
    }
}

#[test]
fn a_database_is_named_after_the_file_and_never_shows_the_query() {
    let host = FakeHost::default();
    configure(&host, URL, false).unwrap();
    let databases = list_databases(&host).unwrap();
    assert_eq!(databases.len(), 1);
    assert_eq!(databases[0].name, "Family vault");
    assert_eq!(databases[0].description, "https://example.org/files/Family%20vault.epdb (read-only)");
    assert!(!databases[0].description.contains("secret"));
    assert_eq!(databases[0].id.len(), 16);
    assert_eq!(find_database(&host, &databases[0].id).unwrap(), databases[0]);
    assert_eq!(find_database(&host, "nope").unwrap_err().code, Code::NotFound);
    // Listing does not use the network (Ease Pass lists at start).
    assert!(host.requests.borrow().is_empty());
}

#[test]
fn several_addresses_and_names_without_a_file() {
    let host = FakeHost::default();
    configure(&host, "  https://a.example/x.epdb\thttp://b.example:8080  https://a.example/x.epdb ", false).unwrap();
    let databases = list_databases(&host).unwrap();
    assert_eq!(databases.len(), 2); // the repeated address counts once
    assert_eq!(databases[0].name, "x");
    assert_eq!(databases[1].name, "b.example:8080");
    assert_eq!(databases[1].description, "http://b.example:8080 (read-only)");
    assert_ne!(databases[0].id, databases[1].id);
}

#[test]
fn addresses_that_are_refused() {
    let host = FakeHost::default();
    for bad in [
        "example.org/x.epdb",
        "ftp://example.org/x.epdb",
        "file:///etc/passwd",
        "https://",
        "https:///x.epdb",
        "https://user:password@example.org/x.epdb",
        &format!("https://example.org/{}", "x".repeat(MAX_URL_LENGTH)),
    ] {
        let error = configure(&host, bad, false).unwrap_err();
        assert_eq!(error.code, Code::InvalidArgument, "{bad}");
    }
    let many: Vec<String> = (0..=MAX_URLS).map(|i| format!("https://example.org/{i}.epdb")).collect();
    assert_eq!(configure(&host, &many.join(" "), false).unwrap_err().code, Code::InvalidArgument);
    // Nothing of that was stored.
    assert!(list_databases(&host).unwrap().is_empty());
    for bad in ["[]", "{}", r#"{"address":1,"offline_copy":false}"#, r#"{"address":"","offline_copy":false,"extra":1}"#] {
        assert_eq!(set_config(&host, bad).unwrap_err().code, Code::InvalidArgument, "{bad}");
    }
}

#[test]
fn reading_downloads_the_file_as_it_is() {
    let host = FakeHost::default();
    configure(&host, URL, false).unwrap();
    let id = list_databases(&host).unwrap()[0].id.clone();
    host.web.borrow_mut().insert(URL.into(), (200, b"encrypted bytes".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap(), b"encrypted bytes");
    assert_eq!(*host.requests.borrow(), vec![URL.to_string()]); // the query is sent
    assert!(host.files.borrow().is_empty()); // no copy unless asked for
    assert_eq!(read_database(&host, "nope").unwrap_err().code, Code::NotFound);
}

#[test]
fn server_answers_become_understandable_errors() {
    let host = FakeHost::default();
    configure(&host, URL, false).unwrap();
    let id = list_databases(&host).unwrap()[0].id.clone();
    assert_eq!(read_database(&host, &id).unwrap_err().code, Code::Network);
    for (status, body, code) in [
        (404, b"x".to_vec(), Code::NotFound),
        (403, b"x".to_vec(), Code::NotAllowed),
        (401, b"x".to_vec(), Code::NotAllowed),
        (500, b"x".to_vec(), Code::Unavailable),
        (204, Vec::new(), Code::Unavailable),
        (200, Vec::new(), Code::Unavailable),
        (200, vec![0u8; MAX_DATABASE_BYTES + 1], Code::TooLarge),
    ] {
        host.web.borrow_mut().insert(URL.into(), (status, body));
        assert_eq!(read_database(&host, &id).unwrap_err().code, code, "status {status}");
    }
}

#[test]
fn offline_copy_is_used_only_when_the_address_fails() {
    let host = FakeHost::default();
    configure(&host, URL, true).unwrap();
    let id = list_databases(&host).unwrap()[0].id.clone();
    // No copy yet: the error is reported.
    assert_eq!(read_database(&host, &id).unwrap_err().code, Code::Network);
    host.web.borrow_mut().insert(URL.into(), (200, b"version 1".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap(), b"version 1");
    host.web.borrow_mut().insert(URL.into(), (200, b"version 2".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap(), b"version 2"); // the server wins while it answers
    host.web.borrow_mut().clear();
    assert_eq!(read_database(&host, &id).unwrap(), b"version 2");
    host.web.borrow_mut().insert(URL.into(), (500, b"oops".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap(), b"version 2"); // an error page never replaces the copy

    // Withdrawn access or a file that became too large is reported, not papered over.
    host.web.borrow_mut().insert(URL.into(), (403, b"no".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap_err().code, Code::NotAllowed);
    host.web.borrow_mut().insert(URL.into(), (200, vec![0u8; MAX_DATABASE_BYTES + 1]));
    assert_eq!(read_database(&host, &id).unwrap_err().code, Code::TooLarge);
    assert_eq!(host.files.borrow().values().next().unwrap(), b"version 2"); // the copy is untouched

    // A copy that cannot be stored does not stop the download.
    *host.fail_writes.borrow_mut() = true;
    host.web.borrow_mut().insert(URL.into(), (200, b"version 3".to_vec()));
    assert_eq!(read_database(&host, &id).unwrap(), b"version 3");
    *host.fail_writes.borrow_mut() = false;

    // Switching the copies off, or removing the address, deletes them.
    configure(&host, URL, false).unwrap();
    assert!(host.files.borrow().is_empty());
    host.web.borrow_mut().clear();
    assert_eq!(read_database(&host, &id).unwrap_err().code, Code::Network);
    configure(&host, URL, true).unwrap();
    host.web.borrow_mut().insert(URL.into(), (200, b"version 4".to_vec()));
    read_database(&host, &id).unwrap();
    assert_eq!(host.files.borrow().len(), 1);
    configure(&host, "https://example.org/other.epdb", true).unwrap();
    assert!(host.files.borrow().is_empty());
}

#[test]
fn writing_is_never_possible() {
    assert_eq!(write_database().code, Code::NotAllowed);
}
