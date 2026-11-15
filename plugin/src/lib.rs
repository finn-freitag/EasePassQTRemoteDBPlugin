//! WebAssembly side of the plugin: connects `easepass_url_database_core` to the plugin
//! interface of Ease Pass Qt (easepass:plugin@1, type `database-source`).

wit_bindgen::generate!({
    path: "../wit",
    world: "easepass:plugin/database-source-plugin",
    generate_all,
});

use easepass::plugin::network::{http_request, Request};
use easepass::plugin::storage;
use easepass::plugin::types::{Error, ErrorCode};
use easepass_url_database_core as core;
use exports::easepass::plugin::database_source::{Availability, DatabaseInfo, Guest};

struct Plugin;
struct WasmHost;

fn from_host(error: Error) -> core::Error {
    let code = match error.code {
        ErrorCode::InvalidArgument => core::Code::InvalidArgument,
        ErrorCode::NotFound => core::Code::NotFound,
        ErrorCode::NotAllowed => core::Code::NotAllowed,
        ErrorCode::TooLarge => core::Code::TooLarge,
        ErrorCode::Network | ErrorCode::Timeout => core::Code::Network,
        ErrorCode::Unavailable => core::Code::Unavailable,
        _ => core::Code::Internal,
    };
    core::Error::new(code, error.message)
}

fn to_host(error: core::Error) -> Error {
    let code = match error.code {
        core::Code::InvalidArgument => ErrorCode::InvalidArgument,
        core::Code::NotFound => ErrorCode::NotFound,
        core::Code::NotAllowed => ErrorCode::NotAllowed,
        core::Code::Unavailable => ErrorCode::Unavailable,
        core::Code::TooLarge => ErrorCode::TooLarge,
        core::Code::Network => ErrorCode::Network,
        core::Code::Internal => ErrorCode::Internal,
    };
    Error { code, message: error.message }
}

impl core::Host for WasmHost {
    fn get_string(&self, key: &str) -> Result<Option<String>, core::Error> {
        storage::get_string(key).map_err(from_host)
    }
    fn set_string(&self, key: &str, value: &str) -> Result<(), core::Error> {
        storage::set_string(key, value).map_err(from_host)
    }
    fn read_file(&self, name: &str) -> Result<Option<Vec<u8>>, core::Error> {
        storage::read_file(name).map_err(from_host)
    }
    fn write_file(&self, name: &str, data: &[u8]) -> Result<(), core::Error> {
        storage::write_file(name, data).map_err(from_host)
    }
    fn delete_file(&self, name: &str) -> Result<(), core::Error> {
        storage::delete_file(name).map_err(from_host)
    }
    fn list_files(&self) -> Result<Vec<String>, core::Error> {
        storage::list_files().map_err(from_host)
    }
    fn http_get(&self, url: &str) -> Result<core::HttpResponse, core::Error> {
        let response = http_request(&Request {
            method: "GET".into(),
            url: url.into(),
            headers: Vec::new(),
            body: None,
            timeout_ms: Some(60_000),
        })
        .map_err(from_host)?;
        Ok(core::HttpResponse { status: response.status, body: response.body })
    }
}

fn info(database: core::Database) -> DatabaseInfo {
    DatabaseInfo {
        id: database.id,
        name: database.name,
        description: database.description,
        read_only: true,
        // Not asked here: listing must not use the network.
        availability: Availability::Unknown,
        last_modified: None,
    }
}

impl Guest for Plugin {
    fn config_schema() -> String {
        core::CONFIG_SCHEMA.into()
    }
    fn get_config() -> Result<String, Error> {
        core::get_config(&WasmHost).map_err(to_host)
    }
    fn set_config(config_json: String) -> Result<(), Error> {
        core::set_config(&WasmHost, &config_json).map_err(to_host)
    }
    fn list_databases() -> Result<Vec<DatabaseInfo>, Error> {
        Ok(core::list_databases(&WasmHost).map_err(to_host)?.into_iter().map(info).collect())
    }
    fn get_database(id: String) -> Result<DatabaseInfo, Error> {
        core::find_database(&WasmHost, &id).map(info).map_err(to_host)
    }
    fn read_database(id: String) -> Result<Vec<u8>, Error> {
        core::read_database(&WasmHost, &id).map_err(to_host)
    }
    fn write_database(_id: String, _data: Vec<u8>) -> Result<(), Error> {
        Err(to_host(core::write_database()))
    }
    fn login(id: String) -> Result<(), Error> {
        core::find_database(&WasmHost, &id).map(|_| ()).map_err(to_host)
    }
    fn logout(_id: String) -> Result<(), Error> {
        Ok(())
    }
}

export!(Plugin);
