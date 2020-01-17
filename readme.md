# Air Vinyl Server

The Air Vinyl server is responsible for serving the UI and exposing an API to control the current streaming status.

## Installation

Currently, the easiest way to get up and running is to clone this repo and build with Cargo:

```sh
cargo build --release
```

## Usage

In order to start the server you need to point it to a built version of [the UI](https://github.com/air-vinyl/server).

```sh
AIR_VINYL_UI=../ui/build target/release/air-vinyl-server
```
