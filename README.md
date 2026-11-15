# Database from a URL (read-only) for Ease Pass Qt

A plugin for [Ease Pass Qt](https://github.com/finn-freitag/EasePassQt). You give it the web
address of an Ease Pass database file (`https://example.org/files/family.epdb`) and the
database shows up on the login page. It can be opened and read, never changed: the plugin
only downloads.

Typical use: one person maintains a database and puts the file on a web server; everyone
else opens it read-only with the master password.

## Install

In Ease Pass Qt: **Settings > Plugins > Manage**, then install it from the store, or
download `com.finnfreitag.easepass.url-database.epplugin` from the
[releases](https://github.com/finn-freitag/EasePassQTRemoteDatabaseSourcePlugin/releases)
and choose **Install from file**.

Ease Pass shows what the plugin may do before it installs it: keep its own settings and
files, and connect to any server (you choose the address, so it cannot be limited to one).

## Settings

**Settings > Plugins > Remote database settings > Database from a URL (read-only)**

| Field | Meaning |
| --- | --- |
| Address of the database file | `https://...` or `http://...`. Several addresses can be separated by a space; each becomes its own database. |
| Keep a copy for when the address cannot be reached | The last download is kept in the plugin's private storage and opened if the address does not answer. |

The database is named after the file (`family.epdb` becomes "family").

## Good to know

- **Read-only, always.** Ease Pass refuses every change to such a database, and the plugin
  has no code that uploads.
- **Nothing is fetched until you open the database.** Listing the databases (which Ease
  Pass does when it starts) does not use the network.
- **The file is still encrypted.** The plugin never sees your master password; it only
  moves the encrypted file. With plain `http://` someone on the way could swap the file
  for an older copy of it, so prefer `https://`.
- **A query in the address is sent but not shown.** `...?token=abc` works; lists and
  descriptions show the address without it.
- **No user name or password in the address** (`https://user:pw@...` is refused): it would
  show up in lists.
- **Size limit: 2 MiB** per database (a limit of how Ease Pass Qt passes data to plugins).
- **Servers in your own network** (192.168.x.x, localhost, ...) are blocked for plugins by
  default. Switch on "local network" for this plugin on the plugin page if your file is
  on such a server.
- **The kept copy** is not used when the server says "access denied" or when the file
  became too large; those are reported.

## Build and test

Needs Rust with the `wasm32-unknown-unknown` target, `wasm-tools` and Python 3; `nix
develop` provides all of them.

```bash
tools/build.sh
```

This runs the tests, builds the WebAssembly component, checks that it has exactly the
plugin type and permissions of `expected.json` (the reference check of the
[plugin specification](https://github.com/finn-freitag/easepassqt-plugin-spec)) and writes
`dist/com.finnfreitag.easepass.url-database.epplugin` with its SHA-256.

- `core/` the plugin's logic as a normal Rust library, with tests (`cargo test`)
- `plugin/` the WebAssembly side that connects it to Ease Pass
- `wit/`, `spec/`, `tools/check_component.py`, `tools/package.py` copied from the plugin specification

## License

MIT
