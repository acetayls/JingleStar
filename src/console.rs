//! Client réseau des consoles Yamaha (QL, CL, TF, Rivage…) : protocole texte sur TCP 49280.
//!
//! La console envoie une ligne « NOTIFY set <adresse> <x> <y> <valeur> "<texte>" » à chaque
//! changement de paramètre (fader, ON…) et « NOTIFY sscurrent_ex … <n> » au rappel d'une scène.
//!
//! JingleStar ne fait qu'écouter et n'écrit aucun octet sur la connexion : la socket n'est
//! accessible qu'à travers [`link::Link`], qui ne sait que lire.

use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::i18n::tr;
use crate::project::ConsoleSettings;
use crate::remote::InputEvent;

mod link {
    use std::io::Read;
    use std::net::TcpStream;

    /// La connexion, en lecture seule : aucune méthode n'écrit vers la console.
    pub struct Link {
        stream: TcpStream,
    }

    impl Link {
        pub fn new(stream: TcpStream) -> Self {
            Self { stream }
        }

        pub fn set_read_timeout(&self, d: std::time::Duration) -> std::io::Result<()> {
            self.stream.set_read_timeout(Some(d))
        }

        pub fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.stream.read(buf)
        }
    }
}

/// État de la connexion, lu par l'interface.
#[derive(Debug, Clone, Default)]
pub struct ConsoleStatus {
    pub connected: bool,
    /// Modèle annoncé par la console (« QL5 »).
    pub model: Option<String>,
    pub error: Option<String>,
    /// Dernière ligne reçue, pour vérifier ce qu'envoie la console.
    pub last_line: Option<String>,
    /// Heure du dernier message reçu.
    pub last_rx: Option<Instant>,
}

/// Connexion en tâche de fond, rétablie automatiquement ; tout s'arrête quand on la libère.
pub struct ConsoleClient {
    pub events: Receiver<InputEvent>,
    status: Arc<Mutex<ConsoleStatus>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ConsoleClient {
    pub fn start(settings: ConsoleSettings, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, events) = unbounded();
        let status = Arc::new(Mutex::new(ConsoleStatus::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (status.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("console".into())
            .spawn(move || run(settings, tx, st, sp, repaint))
            .expect("thread console");
        Self { events, status, stop, thread: Some(thread) }
    }

    pub fn status(&self) -> ConsoleStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for ConsoleClient {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    settings: ConsoleSettings,
    events: Sender<InputEvent>,
    status: Arc<Mutex<ConsoleStatus>>,
    stop: Arc<AtomicBool>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) {
    let update = |f: &dyn Fn(&mut ConsoleStatus)| {
        if let Ok(mut s) = status.lock() {
            f(&mut s);
        }
        repaint();
    };
    // Attente interruptible avant une nouvelle tentative.
    let pause = |secs: u64| {
        let until = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let stream = (settings.host.trim(), settings.port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())
            .and_then(|mut a| a.next().ok_or_else(|| tr("adresse introuvable", "address not found").to_string()))
            .and_then(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| e.to_string()));
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("{} {}:{} ({e})", tr("Connexion impossible à", "Cannot connect to"), settings.host.trim(), settings.port);
                update(&|s| {
                    s.connected = false;
                    s.error = Some(msg.clone());
                });
                pause(3);
                continue;
            }
        };
        update(&|s| {
            s.connected = true;
            s.error = None;
        });
        let link = link::Link::new(stream);
        let err = session(link, &events, &status, &stop, &*repaint).err();
        update(&|s| {
            s.connected = false;
            s.error = err.clone();
        });
        pause(2);
    }
}

/// Lit les lignes de la console jusqu'à la déconnexion.
fn session(
    mut link: link::Link,
    events: &Sender<InputEvent>,
    status: &Mutex<ConsoleStatus>,
    stop: &AtomicBool,
    repaint: &(dyn Fn() + Send + Sync),
) -> Result<(), String> {
    let lost = |e: std::io::Error| format!("{} ({e})", tr("connexion perdue", "connection lost"));
    link.set_read_timeout(Duration::from_millis(200)).map_err(lost)?;
    let mut values: HashMap<(String, u32, u32), i32> = HashMap::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let n = match link.read(&mut buf) {
            Ok(0) => return Err(tr("la console a fermé la connexion", "the console closed the connection").into()),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(lost(e)),
        };
        pending.extend_from_slice(&buf[..n]);
        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let parsed = parse_line(&line);
            if let Ok(mut s) = status.lock() {
                s.last_rx = Some(Instant::now());
                s.last_line = Some(line.clone());
            }
            match parsed {
                Some(Line::Param { address, x, y, value }) => {
                    let previous = values.insert((address.clone(), x, y), value);
                    let _ = events.send(InputEvent::Console { address, x, y, value, previous });
                }
                Some(Line::Scene(number)) => {
                    let _ = events.send(InputEvent::Scene { number });
                }
                _ => {}
            }
            repaint();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Param { address: String, x: u32, y: u32, value: i32 },
    Scene(u32),
}

/// Mots de la ligne ; un texte entre guillemets compte pour un mot.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

pub fn parse_line(line: &str) -> Option<Line> {
    let t = tokens(line);
    let t: Vec<&str> = t.iter().map(String::as_str).collect();
    match t.as_slice() {
        // « OK set » : réponse à une modification faite par un autre client sur la même connexion.
        ["NOTIFY" | "OK", "set", address, x, y, value, ..] => Some(Line::Param {
            address: address.to_string(),
            x: x.parse().ok()?,
            y: y.parse().ok()?,
            value: value.parse().ok()?,
        }),
        ["NOTIFY", cmd, .., n] if cmd.starts_with("sscurrent") || cmd.starts_with("ssrecall") => n.parse().ok().map(Line::Scene),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse_line(r#"NOTIFY set MIXER:Current/InCh/Fader/On 31 0 1 "ON""#),
            Some(Line::Param { address: "MIXER:Current/InCh/Fader/On".into(), x: 31, y: 0, value: 1 })
        );
        assert_eq!(
            parse_line(r#"NOTIFY set MIXER:Current/InCh/Fader/Level 4 0 -32768 "-Inf""#),
            Some(Line::Param { address: "MIXER:Current/InCh/Fader/Level".into(), x: 4, y: 0, value: -32768 })
        );
        assert_eq!(parse_line("NOTIFY sscurrent_ex MIXER:Lib/Scene 12"), Some(Line::Scene(12)));
        assert_eq!(parse_line("NOTIFY sscurrent_ex scene_a 3"), Some(Line::Scene(3)));
        assert_eq!(parse_line("ERROR unknown command"), None);
        assert_eq!(tokens(r#"a "b c" d"#), vec!["a", "b c", "d"]);
    }

    /// Fausse console : envoie quelques changements puis renvoie tout ce que JingleStar a écrit.
    fn fake_console(listener: TcpListener) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.write_all(b"NOTIFY set MIXER:Current/InCh/Fader/On 2 0 0 \"OFF\"\nNOTIFY set MIXER:Cur").unwrap();
            sock.flush().unwrap();
            std::thread::sleep(Duration::from_millis(50));
            sock.write_all(b"rent/InCh/Fader/On 2 0 1 \"ON\"\r\nNOTIFY sscurrent_ex MIXER:Lib/Scene 7\n").unwrap();
            // Recueille ce que le client envoie pendant 1,5 s.
            sock.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let mut received = Vec::new();
            let until = Instant::now() + Duration::from_millis(1500);
            let mut buf = [0u8; 256];
            while Instant::now() < until {
                match sock.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => received.extend_from_slice(&buf[..n]),
                    Err(_) => {}
                }
            }
            received
        })
    }

    fn client(port: u16) -> ConsoleClient {
        let settings = ConsoleSettings { enabled: true, host: "127.0.0.1".into(), port };
        ConsoleClient::start(settings, Arc::new(|| {}))
    }

    #[test]
    fn receives_from_console() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = fake_console(listener);
        let client = client(port);
        let next = || client.events.recv_timeout(Duration::from_secs(3)).unwrap();
        let on = |value, previous| InputEvent::Console { address: "MIXER:Current/InCh/Fader/On".into(), x: 2, y: 0, value, previous };
        assert_eq!(next(), on(0, None));
        assert_eq!(next(), on(1, Some(0)));
        assert_eq!(next(), InputEvent::Scene { number: 7 });
        assert!(client.status().connected);
        // La console n'a reçu aucun octet.
        assert_eq!(fake.join().unwrap(), b"");
    }
}
