use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;

use log::warn;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{RwLock};
use raop_play::{Codec, Frames, MAX_SAMPLES_PER_CHUNK, MetaDataItem, RaopClient, RaopParams, SampleRate, Volume};

fn open_recorder() -> io::Result<(Child, ChildStdout)> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("sox");
    #[cfg(target_os = "macos")]
    cmd.arg("--no-show-progress")
        .arg("--default-device")
        .arg("--encoding")
        .arg("signed-integer")
        .arg("--channels")
        .arg("2")
        .arg("--bits")
        .arg("16")
        .arg("--endian")
        .arg("little")
        .arg("--rate")
        .arg("44100")
        .arg("--type")
        .arg("raw")
        .arg("-");

    #[cfg(not(target_os = "macos"))]
    let mut cmd = Command::new("arecord");
    #[cfg(not(target_os = "macos"))]
    cmd.arg("-t").arg("raw").arg("-f").arg("cd").arg("--device=hw:1,0");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().unwrap();

    Ok((child, stdout))
}

struct Inner {
    addr: Option<SocketAddr>,
    volume: Option<u8>,
}

#[derive(Clone)]
pub struct Streamer {
    inner: Arc<RwLock<Inner>>,
}

impl Streamer {
    pub fn new() -> Streamer {
        let inner = Arc::new(RwLock::new(Inner { addr: None, volume: None }));
        tokio::spawn(Streamer::run(inner.clone()));
        Streamer { inner }
    }

    pub async fn addr(&self) -> Option<SocketAddr> {
        self.inner.read().await.addr
    }

    pub async fn volume(&self) -> Option<u8> {
        self.inner.read().await.volume
    }

    pub async fn update(&self, addr: Option<SocketAddr>, volume: Option<u8>) {
        let mut inner = self.inner.write().await;

        inner.addr = addr;
        inner.volume = volume;
    }

    async fn run(data: Arc<RwLock<Inner>>) {
        let mut clients = HashMap::<SocketAddr, RaopClient>::new();
        let (_, mut stream) = open_recorder().unwrap();
        let mut buf = [0; MAX_SAMPLES_PER_CHUNK.as_usize(4)];
        let mut volume = Option::<Volume>::None;

        loop {
            let (addr, desired_volume) = {
                let guard = data.read().await;
                (guard.addr, guard.volume.map(Volume::from_percent))
            };

            if let Some(desired_volume) = desired_volume {
                if Some(desired_volume) != volume {
                    for client in clients.values_mut() {
                        client.set_volume(desired_volume).await.unwrap();
                    }
                }

                volume = Some(desired_volume);
            }

            if let Some(addr) = addr {
                if !clients.contains_key(&addr) {
                    let mut params = RaopParams::new();

                    params.set_codec(Codec::new(true, MAX_SAMPLES_PER_CHUNK, SampleRate::Hz44100, 16, 2));
                    params.set_desired_latency(Frames::new(44100));

                    let client = RaopClient::connect(params, addr).await.unwrap();

                    if let Some(volume) = volume {
                        client.set_volume(volume).await.unwrap();
                    }

                    let meta_data = MetaDataItem::listing_item(vec![
                        MetaDataItem::item_kind(2),
                    ]);

                    if let Err(err) = client.set_meta_data(meta_data).await {
                        warn!("Failed to set meta data: {}", err);
                    }

                    clients.insert(addr, client);
                }
            }

            let n = stream.read(&mut buf).await.unwrap();

            for client in clients.values_mut() {
                client.accept_frames().await.unwrap();
                client.send_chunk(&buf[0..n]).await.unwrap();
            }

            if addr.is_none() {
                for (_, client) in clients.drain() {
                    client.teardown().await.unwrap();
                }
            }
        }
    }
}
