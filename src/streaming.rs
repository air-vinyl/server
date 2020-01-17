use std::io;
use std::net::SocketAddr;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, RwLock};

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

fn open_player(stream: ChildStdout, addr: SocketAddr, volume: Option<u16>, debug_level: usize) -> io::Result<Child> {
    let d = format!("{}", debug_level);
    let port = format!("{}", addr.port());
    let ip = format!("{}", addr.ip());
    let mut cmd = Command::new("./raop_play");
    cmd.arg("-d").arg(d).arg("-a");
    if let Some(volume) = volume {
        cmd.arg("-v").arg(format!("{}", volume));
    }
    cmd.arg("-p").arg(port).arg(ip).arg("-");
    cmd.stdin(stream);
    cmd.stderr(Stdio::inherit());
    cmd.spawn()
}

struct Inner {
    addr: Option<SocketAddr>,
    debug_level: usize,
    recorder: Option<Child>,
    player: Option<Child>,
    volume: Option<u16>,
}

#[derive(Clone)]
pub struct Streamer {
    inner: Arc<RwLock<Inner>>,
}

impl Streamer {
    pub fn new(debug_level: usize) -> Streamer {
        Streamer {
            inner: Arc::new(RwLock::new(Inner {
                addr: None,
                debug_level,
                recorder: None,
                player: None,
                volume: None,
            })),
        }
    }

    pub fn addr(&self) -> Option<SocketAddr> {
        self.inner.read().unwrap().addr
    }

    pub fn volume(&self) -> Option<u16> {
        self.inner.read().unwrap().volume
    }

    pub fn update(&self, addr: Option<SocketAddr>, volume: Option<u16>) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        let mut inner = self.inner.write().unwrap();

        inner.addr = addr;
        inner.volume = volume;

        if let Some(mut recorder) = inner.recorder.take() {
            // Stop reading audio data
            recorder.kill()?;
        }

        if let Some(mut player) = inner.player.take() {
            // The player will teardown the connection when audio stops, and then terminate
            player.wait()?;
        }

        if let Some(addr) = addr {
            let (recorder, stream) = open_recorder()?;
            let player = open_player(stream, addr, volume, inner.debug_level)?;

            inner.recorder = Some(recorder);
            inner.player = Some(player);
        }

        Ok(())
    }
}
