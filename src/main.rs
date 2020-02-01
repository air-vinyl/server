use docopt::Docopt;
use log::info;
use serde::{Deserialize, Serialize};
use warp::Filter;

use std::env;

mod discovery;
mod streaming;

const USAGE: &str = "
Usage:
    air-vinyl-server [options]
    air-vinyl-server (-h | --help)

Options:
    -d LEVEL      Debug level (0 = silent, 5 = trace) [default: 2]
    -h, --help    Print this help and exit
";

#[derive(Deserialize)]
struct Args {
    flag_d: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct GetResult {
    device: Option<String>,
    volume: Option<u16>,
    devices: Vec<discovery::Device>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct PutInput {
    device: Option<String>,
    volume: Option<u16>,
}

async fn api_get(scanner: discovery::Scanner, streamer: streaming::Streamer) -> Result<impl warp::Reply, warp::Rejection> {
    let addr = streamer.addr();
    let volume = streamer.volume();

    let source = scanner.read_devices();
    let device = source.iter().find(|(_, device)| Some(device.addr) == addr).map(|(id, _)| id.to_owned());
    let devices = source.iter().map(|(_, device)| device.clone()).collect();

    Ok(warp::reply::json(&GetResult { device, volume, devices }))
}

async fn api_put(input: PutInput, scanner: discovery::Scanner, streamer: streaming::Streamer) -> Result<impl warp::Reply, warp::Rejection> {
    info!("PUT /api {:?}", input);

    let addr = input.device.and_then(|id| scanner.device(&id)).map(|device| device.addr);

    // FIXME: Handle errors!
    streamer.update(addr, input.volume).unwrap();

    Ok("test")
}

#[tokio::main]
async fn main() {
    let ui_path = env::var("AIR_VINYL_UI").expect("Please set the env variable AIR_VINYL_UI with a path to the built UI");
    let port = env::var("PORT").map(|p| p.parse::<u16>().expect("The env variable PORT did not contain a valid port number")).unwrap_or(3030);

    let args: Args = Docopt::new(USAGE).and_then(|d| d.deserialize()).unwrap_or_else(|e| e.exit());

    stderrlog::new()
        .verbosity(args.flag_d)
        .timestamp(stderrlog::Timestamp::Microsecond)
        .color(stderrlog::ColorChoice::Never)
        .init()
        .unwrap();

    let scanner = discovery::Scanner::new();
    let scanner = warp::any().map(move || scanner.clone());

    let streamer = streaming::Streamer::new(args.flag_d);
    let streamer = warp::any().map(move || streamer.clone());

    let json_body = warp::body::content_length_limit(1024 * 16).and(warp::body::json());

    // API endpoints
    let api = warp::path("api").and(warp::path::end()).and(scanner).and(streamer);
    let get = warp::get().and(api.clone()).and_then(api_get);
    let put = warp::put().and(json_body).and(api).and_then(api_put);

    // Serve UI
    let ui = warp::fs::dir(ui_path);

    // Spawn the server on the Tokio runtime
    warp::serve(get.or(put).or(ui)).run(([0, 0, 0, 0], port)).await;
}
