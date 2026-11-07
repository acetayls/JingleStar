//! Banc d'audit du décodage et de la lecture en flux, sur de vrais fichiers.
//!
//! Les fichiers sont générés hors du dépôt (générateur à part) avec un signal connu, décrit
//! dans `manifest.json`. Tests ignorés par défaut ; pour les lancer :
//! `JS_AUDIT_DIR=<dossier> cargo test --release audit_tests -- --ignored --nocapture --test-threads=1`
//! (un seul fil : les tests lisent le compteur global de retards disque).

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};

use super::buffer::AudioBuffer;
use super::decode::decode;
use star_core::eq::EqCoeffs;
use super::mixer::{Command, Event, Mixer, Region, Telemetry};
use super::stream::{take_underrun_frames, BLOCK_FRAMES};

const SR: u32 = 48_000;

fn dir() -> Option<PathBuf> {
    std::env::var("JS_AUDIT_DIR").ok().map(PathBuf::from)
}

/// Même formule que le générateur.
fn sig(ch: usize, t: f64) -> f64 {
    let env = 0.6 + 0.4 * (TAU * 3.0 * t).sin();
    let s = |f: f64| (TAU * f * t).sin();
    let v = match ch {
        0 => 0.3 * s(220.0) + 0.2 * s(554.37) + 0.1 * s(1318.51),
        1 => 0.3 * s(330.0) + 0.2 * s(830.61) + 0.1 * s(1975.53),
        2 => 0.3 * s(440.0),
        3 => 0.3 * s(50.0),
        4 => 0.3 * s(700.0),
        _ => 0.3 * s(900.0),
    };
    env * v
}

/// Stéréo attendue après le passage en stéréo du décodeur.
fn reference(channels: usize, t: f64) -> (f64, f64) {
    match channels {
        1 => (sig(0, t), sig(0, t)),
        2 => (sig(0, t), sig(1, t)),
        _ => {
            let h = std::f64::consts::FRAC_1_SQRT_2;
            let norm = 1.0 + 2.0 * h;
            ((sig(0, t) + h * sig(2, t) + h * sig(4, t)) / norm, (sig(1, t) + h * sig(2, t) + h * sig(5, t)) / norm)
        }
    }
}

// ------------------------------------------------------------------ mémoire et CPU (Windows)

#[repr(C)]
#[derive(Default)]
struct Pmc {
    cb: u32,
    page_fault_count: u32,
    peak_working_set: usize,
    working_set: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    pagefile: usize,
    peak_pagefile: usize,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> isize;
    fn K32GetProcessMemoryInfo(h: isize, p: *mut Pmc, cb: u32) -> i32;
    fn GetProcessTimes(h: isize, c: *mut u64, e: *mut u64, k: *mut u64, u: *mut u64) -> i32;
}

/// (mémoire privée engagée, pic de mémoire privée), en Mo.
fn memory_mb() -> (f64, f64) {
    #[cfg(windows)]
    unsafe {
        let mut p = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb);
        (p.pagefile as f64 / 1e6, p.peak_pagefile as f64 / 1e6)
    }
    #[cfg(not(windows))]
    (0.0, 0.0)
}

/// Temps CPU du processus (noyau + utilisateur), en secondes.
fn cpu_secs() -> f64 {
    #[cfg(windows)]
    unsafe {
        let (mut c, mut e, mut k, mut u) = (0, 0, 0, 0);
        GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u);
        (k + u) as f64 / 1e7
    }
    #[cfg(not(windows))]
    0.0
}

// ------------------------------------------------------------------ lecture d'un son

/// Toutes les images d'un son, en attendant le disque pour un son lu en flux.
fn read_all(b: &AudioBuffer) -> (Vec<f32>, Vec<f32>) {
    let mut l = Vec::with_capacity(b.frames);
    let mut r = Vec::with_capacity(b.frames);
    let Some(s) = b.stream.as_deref() else {
        for p in 0..b.frames {
            let (a, c) = b.frame_at(p);
            l.push(a);
            r.push(c);
        }
        return (l, r);
    };
    let mut slot = None;
    let _ = take_underrun_frames();
    for p in 0..b.frames {
        if p % BLOCK_FRAMES == 0 {
            slot = s.publish(slot, p, None, None);
        }
        let t = Instant::now();
        loop {
            let v = s.frame(p);
            if take_underrun_frames() == 0 {
                l.push(v.0);
                r.push(v.1);
                break;
            }
            slot = s.publish(slot, p, None, None);
            assert!(t.elapsed() < Duration::from_secs(5), "bloc jamais chargé à l'image {p}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    s.release(slot);
    (l, r)
}

struct Quality {
    lag: i64,
    gain_db: f64,
    snr_db: f64,
    max_err: f64,
    /// Erreur maximale sur les 20 premières et dernières millisecondes.
    edge_err: f64,
}

/// Compare au signal attendu : retard (corrélation), gain ajusté, rapport signal/erreur.
fn compare(l: &[f32], r: &[f32], rate: u32, channels: usize) -> Quality {
    let n = l.len();
    let refs: Vec<(f64, f64)> = (0..n + 8000).map(|i| reference(channels, i as f64 / rate as f64)).collect();
    // Retard : corrélation normalisée sur une demi-seconde au milieu, de -4000 à +4000 images.
    let w = (rate as usize / 2).min(n / 4);
    let mid = n / 2 - w / 2;
    let mut best = (f64::MIN, 0i64);
    for lag in -4000i64..=4000 {
        let (mut acc, mut e) = (0.0, 0.0);
        for i in (mid..mid + w).step_by(3) {
            let j = i as i64 - lag;
            if j < 0 || j as usize >= refs.len() {
                continue;
            }
            let (a, b) = refs[j as usize];
            acc += l[i] as f64 * a + r[i] as f64 * b;
            e += a * a + b * b;
        }
        let score = acc / e.max(1e-12).sqrt();
        if score > best.0 {
            best = (score, lag);
        }
    }
    let lag = best.1;
    let margin = (rate as usize / 5).min(n / 4);
    let (mut sx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for i in margin..n - margin {
        let j = (i as i64 - lag) as usize;
        let (a, b) = refs[j];
        sx += a * a + b * b;
        sxy += a * l[i] as f64 + b * r[i] as f64;
        syy += (l[i] as f64).powi(2) + (r[i] as f64).powi(2);
    }
    let _ = syy;
    let g = sxy / sx;
    let (mut err, mut max_err) = (0.0, 0f64);
    for i in margin..n - margin {
        let j = (i as i64 - lag) as usize;
        let (a, b) = refs[j];
        let (ea, eb) = (l[i] as f64 - a * g, r[i] as f64 - b * g);
        err += ea * ea + eb * eb;
        max_err = max_err.max(ea.abs()).max(eb.abs());
    }
    let edge = (rate as usize / 50).min(n / 4);
    let mut edge_err = 0f64;
    for i in (0..edge).chain(n - edge..n) {
        let j = i as i64 - lag;
        if j < 0 {
            continue;
        }
        let (a, b) = refs[j as usize];
        edge_err = edge_err.max((l[i] as f64 - a).abs()).max((r[i] as f64 - b).abs());
    }
    Quality { lag, gain_db: 20.0 * g.log10(), snr_db: 10.0 * (sx * g * g / err.max(1e-30)).log10(), max_err, edge_err }
}

fn manifest(dir: &Path) -> Vec<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap()
}

// ------------------------------------------------------------------ 1. décodage

#[test]
#[ignore]
fn audit_decode_matrix() {
    let Some(dir) = dir() else { return };
    println!("\n{:<34} {:>6} {:>7} {:>7} {:>8} {:>8} {:>9} {:>9} {:>8} {:>6} {}", "fichier", "cible", "images", "écart", "retard", "gain dB", "S/E dB", "err max", "bords", "flux", "avertissement");
    for e in manifest(&dir) {
        if e["codec"] == "corrupt" {
            continue;
        }
        let file = e["file"].as_str().unwrap();
        let channels = e["channels"].as_u64().unwrap() as usize;
        let secs = e["secs"].as_f64().unwrap();
        let src_rate = e["rate"].as_u64().unwrap() as u32;
        let targets: &[u32] = if src_rate == 44_100 && e["lossless"] == true { &[48_000, 44_100] } else { &[48_000] };
        for &target in targets {
            let t = Instant::now();
            let d = match decode(&dir.join(file), target, false) {
                Ok(d) => d,
                Err(err) => {
                    println!("{file:<34} {target:>6} ÉCHEC {err}");
                    continue;
                }
            };
            let took = t.elapsed();
            let b = &d.buffer;
            let (l, r) = read_all(b);
            let q = compare(&l, &r, target, channels);
            let expected = (secs * target as f64).round() as i64;
            println!(
                "{file:<34} {target:>6} {:>7} {:>+7} {:>+8} {:>8.3} {:>9.1} {:>9.2e} {:>8.1e} {:>6} {} ({:.0} ms)",
                b.frames,
                b.frames as i64 - expected,
                q.lag,
                q.gain_db,
                q.snr_db,
                q.max_err,
                q.edge_err,
                if b.stream.is_some() { "oui" } else { "non" },
                d.warning.clone().unwrap_or_default(),
                took.as_secs_f64() * 1000.0
            );
            assert_eq!(b.stream.is_some(), b.duration_secs() > 30.0, "{file} : flux attendu au-delà de 30 s");
        }
    }
}

// ------------------------------------------------------------------ 2. fichiers abîmés

#[test]
#[ignore]
fn audit_broken_files() {
    let Some(dir) = dir() else { return };
    println!();
    for e in manifest(&dir) {
        if e["codec"] != "corrupt" {
            continue;
        }
        let file = e["file"].as_str().unwrap().to_string();
        let path = dir.join(&file);
        let (tx, rx) = bounded(1);
        std::thread::spawn(move || {
            let t = Instant::now();
            let r = std::panic::catch_unwind(|| decode(&path, SR, false));
            let _ = tx.send((r, t.elapsed()));
        });
        match rx.recv_timeout(Duration::from_secs(20)) {
            Err(_) => println!("{file:<26} BLOQUÉ (plus de 20 s)"),
            Ok((Err(_), t)) => println!("{file:<26} PANIQUE ({:.0} ms)", t.as_secs_f64() * 1000.0),
            Ok((Ok(Err(err)), t)) => println!("{file:<26} refusé ({:.0} ms) : {err}", t.as_secs_f64() * 1000.0),
            Ok((Ok(Ok(d)), t)) => println!(
                "{file:<26} lu ({:.0} ms) : {:.2} s, avertissement : {}  [{}]",
                t.as_secs_f64() * 1000.0,
                d.buffer.duration_secs(),
                d.warning.unwrap_or_else(|| "AUCUN".into()),
                e["note"].as_str().unwrap()
            ),
        }
    }
}

// ------------------------------------------------------------------ 3. lecture en flux en temps réel

/// Mixeur rendu au rythme d'une carte son (blocs de 480 images toutes les 10 ms).
struct Rig {
    mixer: Mixer,
    tx: Sender<Command>,
    _events: Receiver<Event>,
    _garbage: Receiver<Arc<AudioBuffer>>,
}

impl Rig {
    fn new() -> Self {
        let (tx, rx) = bounded(4096);
        let (etx, erx) = bounded(1024);
        let (gtx, grx) = bounded(1024);
        Self { mixer: Mixer::new(2, SR, rx, etx, gtx, Arc::new(Telemetry::new())), tx, _events: erx, _garbage: grx }
    }
}

#[derive(Clone, Copy, Debug)]
enum Act {
    Play { slot: usize, start: usize, looping: Option<(usize, usize)> },
    Seek { slot: usize, frame: usize },
    Pause { slot: usize, paused: bool },
    Stop { slot: usize },
    /// Le callback audio ne tourne plus pendant ce nombre de millisecondes.
    Stall(u64),
}

struct Outcome {
    underrun_ms: f64,
    diff_frames: usize,
    first_diff_ms: Option<f64>,
    max_d2_jump: f32,
}

/// Joue le même scénario sur un son lu en flux et sur sa copie en mémoire, en temps réel,
/// et compare les sorties image par image.
fn run(streams: &[Arc<AudioBuffer>], twin: &Arc<AudioBuffer>, script: &[(u64, Act)], total_ms: u64, pins: &[usize]) -> Outcome {
    for s in streams {
        s.set_pins(pins.to_vec());
    }
    // Laisse le fil de fond charger les points de départ.
    std::thread::sleep(Duration::from_millis(300));
    let mut a = Rig::new();
    let mut b = Rig::new();
    let _ = take_underrun_frames();
    let block = 480;
    let mut out_a = vec![0f32; block * 2];
    let mut out_b = vec![0f32; block * 2];
    let (mut diff, mut first, mut d2) = (0usize, None, 0f32);
    let mut prev_a = [0f32; 2];
    let mut prev2_a = [0f32; 2];
    let mut prev_b = [0f32; 2];
    let mut prev2_b = [0f32; 2];
    let mut start = Instant::now();
    let mut next = 0;
    let mut t_ms = 0u64;
    let mut frame_no = 0usize;
    while t_ms < total_ms {
        while next < script.len() && script[next].0 <= t_ms {
            let act = script[next].1;
            next += 1;
            let cmd = |buf: &Arc<AudioBuffer>| match act {
                Act::Play { slot, start, looping } => Some(Command::Play {
                    module: 0,
                    slot,
                    id: slot as u64 + 1,
                    buffer: buf.clone(),
                    gain: 1.0,
                    region: Region { start, end: buf.frames, looping, fade_in: 0, fade_out: 0 },
                    eq: EqCoeffs::flat(),
                }),
                Act::Seek { slot, frame } => Some(Command::Seek { module: 0, slot, frame }),
                Act::Pause { slot, paused } => Some(Command::SetPaused { module: 0, slot, paused }),
                Act::Stop { slot } => Some(Command::Stop { module: 0, slot }),
                Act::Stall(_) => None,
            };
            let slot = match act {
                Act::Play { slot, .. } | Act::Seek { slot, .. } | Act::Pause { slot, .. } | Act::Stop { slot } => slot,
                Act::Stall(_) => 0,
            };
            if let Act::Stall(ms) = act {
                // Pas de rattrapage ensuite : un pilote reprend au rythme normal.
                std::thread::sleep(Duration::from_millis(ms));
                start += Duration::from_millis(ms);
                continue;
            }
            if let Some(c) = cmd(&streams[slot % streams.len()]) {
                a.tx.send(c).unwrap();
            }
            if let Some(c) = cmd(twin) {
                b.tx.send(c).unwrap();
            }
        }
        a.mixer.process(&mut out_a);
        b.mixer.process(&mut out_b);
        for f in 0..block {
            for c in 0..2 {
                let (x, y) = (out_a[f * 2 + c], out_b[f * 2 + c]);
                if (x - y).abs() > 1e-6 {
                    diff += 1;
                    first.get_or_insert(frame_no as f64 * 1000.0 / SR as f64);
                }
                let da = (x - 2.0 * prev_a[c] + prev2_a[c]).abs();
                let db = (y - 2.0 * prev_b[c] + prev2_b[c]).abs();
                d2 = d2.max(da - db);
                (prev2_a[c], prev_a[c]) = (prev_a[c], x);
                (prev2_b[c], prev_b[c]) = (prev_b[c], y);
            }
            frame_no += 1;
        }
        t_ms += 10;
        // Rythme réel, comme un pilote.
        let due = start + Duration::from_millis(t_ms);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    Outcome {
        underrun_ms: take_underrun_frames() as f64 * 1000.0 / SR as f64,
        diff_frames: diff / 2,
        first_diff_ms: first,
        max_d2_jump: d2,
    }
}

fn report(name: &str, o: &Outcome) {
    println!(
        "{name:<58} silence disque {:>7.1} ms | images différentes de la copie en mémoire {:>6} (première à {}) | saut de dérivée seconde {:.3}",
        o.underrun_ms,
        o.diff_frames,
        o.first_diff_ms.map_or("-".into(), |t| format!("{t:.0} ms")),
        o.max_d2_jump
    );
}

fn long_sound(dir: &Path) -> (Arc<AudioBuffer>, Arc<AudioBuffer>) {
    let d = decode(&dir.join("wavf32_48000_2ch_75s.wav"), SR, false).unwrap();
    assert!(d.buffer.stream.is_some());
    let (l, r) = read_all(&d.buffer);
    let twin = AudioBuffer::from_planar(l, r, SR);
    assert!(twin.stream.is_none());
    (Arc::new(d.buffer), Arc::new(twin))
}

#[test]
#[ignore]
fn audit_streaming_realtime() {
    let Some(dir) = dir() else { return };
    let (s, twin) = long_sound(&dir);
    let st = std::slice::from_ref(&s);
    let sec = |x: f64| (x * SR as f64) as usize;
    println!();
    let none = Act::Play { slot: 0, start: 0, looping: None };
    report("A. départ au début, 3 s", &run(st, &twin, &[(0, none)], 3000, &[]));
    report("B. départ à 20 s sans point gardé (pins)", &run(st, &twin, &[(0, Act::Play { slot: 0, start: sec(20.0), looping: None })], 2000, &[]));
    report("C. départ à 21 s avec point gardé", &run(st, &twin, &[(0, Act::Play { slot: 0, start: sec(21.0), looping: None })], 2000, &[sec(21.0)]));
    report(
        "D. lecture puis déplacement à 60 s",
        &run(st, &twin, &[(0, none), (1000, Act::Seek { slot: 0, frame: sec(60.0) })], 3000, &[]),
    );
    let mut storm = vec![(0, none)];
    for i in 0..40u64 {
        storm.push((100 + i * 75, Act::Seek { slot: 0, frame: sec(((i * 37) % 70) as f64 + 0.37) }));
    }
    report("E. 40 déplacements rapprochés (toutes les 75 ms)", &run(st, &twin, &storm, 4000, &[]));
    report(
        "F. boucle 40-44 s (début de boucle gardé), départ à 39 s, 10 s",
        &run(st, &twin, &[(0, Act::Play { slot: 0, start: sec(39.0), looping: Some((sec(40.0), sec(44.0))) })], 10_000, &[sec(39.0), sec(40.0)]),
    );
    report(
        "G. pause 4 s puis reprise",
        &run(st, &twin, &[(0, none), (500, Act::Pause { slot: 0, paused: true }), (4500, Act::Pause { slot: 0, paused: false })], 6000, &[]),
    );
    report(
        "H. callback figé 3 s (au-delà de READER_TIMEOUT) puis reprise",
        &run(st, &twin, &[(0, Act::Play { slot: 0, start: 0, looping: None }), (1000, Act::Seek { slot: 0, frame: sec(30.0) }), (1500, Act::Stall(3000))], 4000, &[]),
    );
    report(
        "I. arrêt en fondu pendant la lecture à 50 s",
        &run(st, &twin, &[(0, Act::Play { slot: 0, start: sec(50.0), looping: None }), (800, Act::Stop { slot: 0 })], 1500, &[sec(50.0)]),
    );
}

#[test]
#[ignore]
fn audit_streaming_many_voices() {
    let Some(dir) = dir() else { return };
    let (_, twin) = long_sound(&dir);
    // 8 sons distincts (8 fichiers temporaires), joués ensemble avec déplacements.
    let streams: Vec<Arc<AudioBuffer>> =
        (0..8).map(|_| Arc::new(decode(&dir.join("wavf32_48000_2ch_75s.wav"), SR, false).unwrap().buffer)).collect();
    let sec = |x: f64| (x * SR as f64) as usize;
    let mut script = Vec::new();
    for v in 0..8 {
        script.push((v as u64 * 10, Act::Play { slot: v, start: 0, looping: None }));
        script.push((500 + v as u64 * 130, Act::Seek { slot: v, frame: sec(10.0 + v as f64 * 7.3) }));
        script.push((2500 + v as u64 * 90, Act::Seek { slot: v, frame: sec(65.0 - v as f64 * 5.1) }));
    }
    println!();
    report("J. 8 sons longs ensemble, 16 déplacements", &run(&streams, &twin, &script, 4000, &[]));
}

// ------------------------------------------------------------------ 4. mémoire, disque, CPU

fn write_long_wav(path: &Path, rate: u32, secs: f64) {
    use std::io::Write;
    let n = (secs * rate as f64) as usize;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let data = (n * 4) as u32;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&rate.to_le_bytes()).unwrap();
    f.write_all(&(rate * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data.to_le_bytes()).unwrap();
    for i in 0..n {
        let t = i as f64 / rate as f64;
        f.write_all(&((sig(0, t) * 32767.0) as i16).to_le_bytes()).unwrap();
        f.write_all(&((sig(1, t) * 32767.0) as i16).to_le_bytes()).unwrap();
    }
}

fn flux_dir_bytes() -> (usize, u64) {
    let dir = directories::ProjectDirs::from("com", "acetayls", "JingleStar").map(|d| d.cache_dir().join("flux")).unwrap();
    let mut n = 0;
    let mut bytes = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            n += 1;
            bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    (n, bytes)
}

/// Pic de mémoire au décodage d'un long son (à lancer seul : le pic est celui du processus).
#[test]
#[ignore]
fn audit_memory_peak_long_decode() {
    let Some(dir) = dir() else { return };
    let mins: f64 = std::env::var("JS_AUDIT_MINUTES").ok().and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let path = dir.join(format!("long_{mins}min_44100.wav"));
    if !path.exists() {
        write_long_wav(&path, 44_100, mins * 60.0);
    }
    let (base, _) = memory_mb();
    let (files0, bytes0) = flux_dir_bytes();
    let t = Instant::now();
    let d = decode(&path, SR, false).unwrap();
    let took = t.elapsed();
    let (after, peak) = memory_mb();
    let decoded_mb = d.buffer.frames as f64 * 8.0 / 1e6;
    let (files1, bytes1) = flux_dir_bytes();
    println!(
        "\nson de {mins} min 44,1 kHz → 48 kHz : décodé en {:.1} s ; son décodé entier = {decoded_mb:.0} Mo ; \
         mémoire avant {base:.0} Mo, pic {peak:.0} Mo (pic au-dessus du départ : {:.0} Mo = {:.1} × le son), \
         après {after:.0} Mo ; memory_bytes() = {:.1} Mo ; fichier temporaire : +{} fichier(s), +{:.0} Mo",
        took.as_secs_f64(),
        peak - base,
        (peak - base) / decoded_mb,
        d.buffer.memory_bytes() as f64 / 1e6,
        files1 - files0,
        (bytes1 - bytes0) as f64 / 1e6
    );
    drop(d);
    std::thread::sleep(Duration::from_millis(1500));
    let (files2, _) = flux_dir_bytes();
    let (after_drop, _) = memory_mb();
    println!("après libération du son : mémoire {after_drop:.0} Mo, fichiers temporaires {files2} (avant : {files0})");
}

/// Mémoire rendue quand des sons longs sont retirés, et coût CPU du fil de fond.
#[test]
#[ignore]
fn audit_memory_release_and_streamer_cpu() {
    let Some(dir) = dir() else { return };
    let (m0, _) = memory_mb();
    let (f0, _) = flux_dir_bytes();
    let c0 = cpu_secs();
    std::thread::sleep(Duration::from_secs(2));
    let idle_cpu = (cpu_secs() - c0) / 2.0 * 100.0;
    let mut bufs = Vec::new();
    for _ in 0..12 {
        bufs.push(Arc::new(decode(&dir.join("wavf32_48000_2ch_75s.wav"), SR, false).unwrap().buffer));
    }
    let (m1, _) = memory_mb();
    let resident: usize = bufs.iter().map(|b| b.memory_bytes()).sum();
    let (f1, b1) = flux_dir_bytes();
    let c1 = cpu_secs();
    std::thread::sleep(Duration::from_secs(3));
    let cpu12 = (cpu_secs() - c1) / 3.0 * 100.0;
    println!(
        "\n12 sons de 75 s : mémoire +{:.0} Mo (memory_bytes total {:.0} Mo ; en mémoire entière ce serait {:.0} Mo) ; \
         {} fichiers temporaires ({:.0} Mo) ; CPU au repos : {idle_cpu:.2} % sans son, {cpu12:.2} % avec 12 sons",
        m1 - m0,
        resident as f64 / 1e6,
        12.0 * 75.0 * 48_000.0 * 8.0 / 1e6,
        f1 - f0,
        b1 as f64 / 1e6
    );
    // Coût du fil de fond pour un son très long (nombre de blocs) : 1 h à 48 kHz ≈ 5 300 blocs.
    let big: Vec<f32> = vec![0.1; 48_000 * 60 * 20 * 2];
    let huge = super::stream::Stream::create(&big, SR).unwrap();
    let huge2 = super::stream::Stream::create(&big, SR).unwrap();
    drop(big);
    let c2 = cpu_secs();
    std::thread::sleep(Duration::from_secs(3));
    let cpu_big = (cpu_secs() - c2) / 3.0 * 100.0;
    println!("+ 2 sons de 20 min (2 × 1 758 blocs) : CPU au repos {cpu_big:.2} % d'un cœur");
    drop(huge);
    drop(huge2);
    drop(bufs);
    std::thread::sleep(Duration::from_millis(1500));
    let (m2, _) = memory_mb();
    let (f2, _) = flux_dir_bytes();
    println!("tout libéré : mémoire {:+.0} Mo par rapport au départ, fichiers temporaires {f2} (départ {f0})", m2 - m0);
}

/// Durée d'écriture du fichier temporaire d'un son d'une heure, et du décodage seul.
#[test]
#[ignore]
fn audit_temp_write_speed() {
    let Some(dir) = dir() else { return };
    let samples: Vec<f32> = (0..48_000usize * 3600 * 2).map(|i| (i % 1000) as f32 * 1e-4).collect();
    let t = Instant::now();
    let s = super::stream::Stream::create(&samples, SR).unwrap();
    println!("\nécriture du fichier temporaire d'1 h (1 382 Mo) : {:.1} s", t.elapsed().as_secs_f64());
    drop(s);
    drop(samples);
    let path = dir.join("long_60min_44100.wav");
    if path.exists() {
        let t = Instant::now();
        let d = decode(&path, 44_100, false).unwrap();
        println!("décodage 1 h sans rééchantillonnage (44,1 → 44,1) + écriture : {:.1} s", t.elapsed().as_secs_f64());
        drop(d);
        let t = Instant::now();
        let d = decode(&path, SR, false).unwrap();
        println!("décodage 1 h avec rééchantillonnage (44,1 → 48) + écriture : {:.1} s", t.elapsed().as_secs_f64());
        drop(d);
    }
}

/// Retard de départ et longueur avec la suppression des silences d'encodage de symphonia
/// (`enable_gapless`), que le décodeur de l'application n'active pas.
#[test]
#[ignore]
fn audit_gapless_option() {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    let Some(dir) = dir() else { return };
    println!();
    for e in manifest(&dir) {
        let codec = e["codec"].as_str().unwrap();
        if !(codec.starts_with("mp3") || codec.starts_with("aac") || codec == "vorbis" || codec == "flac") || e["secs"].as_f64().unwrap() > 30.0 {
            continue;
        }
        let file = e["file"].as_str().unwrap();
        let rate = e["rate"].as_u64().unwrap() as u32;
        let channels = e["channels"].as_u64().unwrap() as usize;
        for gapless in [false, true] {
            let mss = MediaSourceStream::new(Box::new(std::fs::File::open(dir.join(file)).unwrap()), Default::default());
            let mut hint = symphonia::core::probe::Hint::new();
            hint.with_extension(file.rsplit('.').next().unwrap());
            let opts = FormatOptions { enable_gapless: gapless, ..Default::default() };
            let mut fmt = symphonia::default::get_probe().format(&hint, mss, &opts, &Default::default()).unwrap().format;
            let track = fmt.default_track().unwrap().clone();
            let mut dec = symphonia::default::get_codecs().make(&track.codec_params, &Default::default()).unwrap();
            let (mut l, mut r) = (Vec::new(), Vec::new());
            while let Ok(p) = fmt.next_packet() {
                let Ok(d) = dec.decode(&p) else { continue };
                let ch = d.spec().channels.count();
                let mut sb = SampleBuffer::<f32>::new(d.capacity() as u64, *d.spec());
                sb.copy_interleaved_ref(d);

                for f in sb.samples().chunks_exact(ch) {
                    l.push(f[0]);
                    r.push(if ch > 1 { f[1] } else { f[0] });
                }
            }
            let q = compare(&l, &r, rate, channels);
            println!(
                "{file:<32} gapless={gapless:<5} images {:>7} (attendu {:>7}) retard {:>+5} images = {:>5.1} ms",
                l.len(),
                (e["secs"].as_f64().unwrap() * rate as f64) as usize,
                q.lag,
                q.lag as f64 * 1000.0 / rate as f64
            );
        }
    }
}
