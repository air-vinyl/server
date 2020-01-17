use log::info;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::process::Stdio;
use std::sync::{Arc, RwLock, RwLockReadGuard};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
}

type Devices = HashMap<String, Device>;

#[derive(Clone)]
pub struct Scanner {
    devices: Arc<RwLock<Devices>>,
}

impl Scanner {
    pub fn new() -> Scanner {
        let devices = Arc::new(RwLock::new(HashMap::new()));
        tokio::spawn(scan(Arc::clone(&devices)));
        Scanner { devices }
    }

    pub fn read_devices(&self) -> RwLockReadGuard<Devices> {
        self.devices.read().unwrap()
    }

    pub fn device(&self, id: &str) -> Option<Device> {
        self.read_devices().get(id).map(|device| device.clone())
    }
}

#[cfg(target_os = "macos")]
async fn scan(devices: Arc<RwLock<Devices>>) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    async fn lookup_ip(host: &str) -> Result<IpAddr, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let mut cmd = Command::new("dns-sd");

        cmd.arg("-G").arg("v4").arg(host);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;

        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // Skip first 3 lines
        reader.next_line().await?;
        reader.next_line().await?;
        reader.next_line().await?;

        let info = reader.next_line().await?.unwrap();
        let ip = info[69..84].trim().parse::<IpAddr>()?;

        child.kill()?;

        Ok(ip)
    }

    async fn lookup_device(name: &str) -> Result<Device, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let mut cmd = Command::new("dns-sd");

        cmd.arg("-L").arg(name).arg("_raop");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;

        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout).lines();

        // Skip first 3 lines
        reader.next_line().await?;
        reader.next_line().await?;
        reader.next_line().await?;

        let info = reader.next_line().await?.unwrap();
        let tail = info.split(" can be reached at ").skip(1).next().unwrap();
        let head = tail.split(" ").next().unwrap();
        let mut parts = head.split(":");
        let host = parts.next().unwrap();
        let port = parts.next().unwrap().parse::<u16>()?;

        let ip = lookup_ip(host).await?;
        let addr = SocketAddr::new(ip, port);

        child.kill()?;

        Ok(Device {
            id: name.to_owned(),
            name: name.to_owned(),
            addr,
        })
    }

    let mut cmd = Command::new("dns-sd");

    cmd.arg("-B").arg("_raop");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    // Skip first 4 lines
    reader.next_line().await?;
    reader.next_line().await?;
    reader.next_line().await?;
    reader.next_line().await?;

    while let Some(line) = reader.next_line().await? {
        if line.starts_with("DATE:") {
            continue;
        }

        let op = &line[14..17];
        let name = &line[73..];

        match op {
            "Add" => {
                let device = lookup_device(name).await?;
                let mut devices = devices.write().unwrap();
                if !devices.contains_key(name) {
                    info!("Found AirPlay device {:?}", device);
                    devices.insert(name.to_owned(), device);
                }
            }
            "Rmv" => {
                let mut devices = devices.write().unwrap();
                if devices.contains_key(name) {
                    info!("Lost AirPlay device with name \"{:?}\"", name);
                    devices.remove(name);
                }
            }
            _ => panic!("Invalid dns-sd op: {}", op),
        }
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
async fn scan(devices: Arc<RwLock<Devices>>) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut cmd = Command::new("avahi-browse");

    cmd.arg("-p").arg("-r").arg("_raop._tcp");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    while let Some(line) = reader.next_line().await? {
        let mut parts = line.split(";");

        let op = parts.next().unwrap();
        let _iface = parts.next().unwrap();
        let _ipver = parts.next().unwrap();
        let id = parts.next().unwrap();
        let _type = parts.next().unwrap();
        let _domain = parts.next().unwrap();

        match op {
            "+" => {}
            "=" => {
                let name = parts.next().unwrap();
                let ip: IpAddr = parts.next().unwrap().parse()?;
                let port: u16 = parts.next().unwrap().parse()?;

                let addr = SocketAddr::new(ip, port);
                let device = Device {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    addr,
                };

                let mut devices = devices.write().unwrap();
                if !devices.contains_key(id) {
                    info!("Found AirPlay device {:?}", device);
                    devices.insert(id.to_owned(), device);
                }
            }
            "-" => {
                let mut devices = devices.write().unwrap();
                if devices.contains_key(id) {
                    info!("Lost AirPlay device with name \"{:?}\"", id);
                    devices.remove(id);
                }
            }
            _ => panic!("Invalid dns-sd op: {}", op),
        }
    }

    Ok(())
}
