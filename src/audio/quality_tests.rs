//! Banc d'essai hors ligne du chemin audio : le mixeur est rendu dans des tampons,
//! sans carte son, exactement comme dans le callback (limiteur compris).
//!
//! Critères :
//! - transparence : à 0 dB sans EQ, la sortie est le fichier au bit près ;
//! - clics : la dérivée seconde du signal reste au niveau de celle du sinus de test
//!   (une coupure franche donne une dérivée seconde de l'ordre de l'amplitude) ;
//! - robustesse : aucune valeur non finie, aucune boucle sans fin, aucune panique ;
//! - temps réel : aucune allocation ni libération pendant `Mixer::process`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::f64::consts::PI;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};

use super::buffer::{db_to_gain, AudioBuffer};
use super::decode::decode_file;
use super::eq::{Band, CutSlope, EqCoeffs, EqProcessor, EqSettings, EqState};
use super::mixer::{Command, Event, Mixer, Queued, Region, Telemetry, MAX_MODULES, PLAYLIST_SLOTS, SLOTS};

const SR: u32 = 48_000;

// ------------------------------------------------------------------ outils

/// Compte les allocations du thread courant quand le compteur est armé.
struct CountingAlloc;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ARMED.with(|a| a.get()) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ARMED.with(|a| a.get()) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

struct Rig {
    mixer: Mixer,
    tx: Sender<Command>,
    events: Receiver<Event>,
    _garbage: Receiver<Arc<AudioBuffer>>,
    channels: usize,
}

impl Rig {
    fn new(channels: usize) -> Self {
        // Mêmes files que le moteur (bornées).
        let (tx, rx) = bounded(4096);
        let (etx, erx) = bounded(1024);
        let (gtx, grx) = bounded(1024);
        let mixer = Mixer::new(channels, SR, rx, etx, gtx, Arc::new(Telemetry::new()));
        Self { mixer, tx, events: erx, _garbage: grx, channels }
    }

    fn send(&self, cmd: Command) {
        self.tx.send(cmd).unwrap();
    }

    fn play(&self, slot: usize, buffer: &Arc<AudioBuffer>, gain: f32, region: Region, eq: EqCoeffs) {
        self.send(Command::Play { module: 0, slot, id: slot as u64 + 1, buffer: buffer.clone(), gain, region, eq });
    }

    /// Rend `frames` images par blocs de taille variable (comme un pilote irrégulier).
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * self.channels];
        let sizes = [256, 97, 512, 1, 333, 128];
        let mut pos = 0;
        let mut i = 0;
        while pos < frames {
            let n = sizes[i % sizes.len()].min(frames - pos);
            self.mixer.process(&mut out[pos * self.channels..(pos + n) * self.channels]);
            pos += n;
            i += 1;
        }
        out
    }

    fn latency(&self) -> usize {
        self.mixer.latency()
    }
}

fn buffer(left: Vec<f32>, right: Vec<f32>) -> Arc<AudioBuffer> {
    Arc::new(AudioBuffer::from_planar(left, right, SR))
}

fn sine_vec(freq: f64, amp: f64, frames: usize, phase: f64) -> Vec<f32> {
    (0..frames).map(|i| (amp * (2.0 * PI * freq * i as f64 / SR as f64 + phase).sin()) as f32).collect()
}

fn sine(freq: f64, amp: f64, frames: usize) -> Arc<AudioBuffer> {
    let v = sine_vec(freq, amp, frames, 0.0);
    buffer(v.clone(), v)
}

/// Bruit blanc déterministe (pleine échelle, deux canaux différents).
fn noise(frames: usize, amp: f32) -> Arc<AudioBuffer> {
    let mut seed = 0x1234_5678u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * amp
    };
    let l: Vec<f32> = (0..frames).map(|_| next()).collect();
    let r: Vec<f32> = (0..frames).map(|_| next()).collect();
    buffer(l, r)
}

fn channel(out: &[f32], channels: usize, c: usize) -> Vec<f32> {
    out.iter().skip(c).step_by(channels).copied().collect()
}

/// Plus grande dérivée seconde : ≈ 0 pour un sinus grave, ≈ amplitude pour un clic.
fn max_d2(x: &[f32]) -> f32 {
    x.windows(3).fold(0f32, |a, w| a.max((w[2] - 2.0 * w[1] + w[0]).abs()))
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |a, s| a.max(s.abs()))
}

/// Taux de distorsion harmonique (harmoniques 2 à 10) en dB, sur un nombre entier de périodes.
fn thd_db(x: &[f32], freq: f64) -> f64 {
    let amp = |h: f64| {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in x.iter().enumerate() {
            let w = 2.0 * PI * freq * h * i as f64 / SR as f64;
            re += s as f64 * w.cos();
            im += s as f64 * w.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / x.len() as f64
    };
    let fundamental = amp(1.0);
    let harmonics: f64 = (2..=10).map(|h| amp(h as f64).powi(2)).sum::<f64>().sqrt();
    20.0 * (harmonics.max(1e-30) / fundamental).log10()
}

/// Résidu d'un sinus après retrait de la meilleure sinusoïde à `freq` (THD+N), en dB.
fn thd_n_db(x: &[f32], freq: f64, sr: f64) -> f64 {
    let n = x.len() as f64;
    let (mut a, mut b) = (0.0, 0.0);
    for (i, &s) in x.iter().enumerate() {
        let w = 2.0 * PI * freq * i as f64 / sr;
        a += s as f64 * w.sin();
        b += s as f64 * w.cos();
    }
    let (a, b) = (a * 2.0 / n, b * 2.0 / n);
    let mut res = 0.0;
    let mut sig = 0.0;
    for (i, &s) in x.iter().enumerate() {
        let w = 2.0 * PI * freq * i as f64 / sr;
        let fit = a * w.sin() + b * w.cos();
        res += (s as f64 - fit).powi(2);
        sig += fit * fit;
    }
    10.0 * (res.max(1e-30) / sig).log10()
}

fn assert_finite(out: &[f32]) {
    assert!(out.iter().all(|s| s.is_finite()), "valeur non finie en sortie");
}

// ------------------------------------------------------- transparence, THD

#[test]
fn bit_transparent_at_0db_without_eq() {
    let src = noise(20_000, 0.999);
    for slot in [0, 5, PLAYLIST_SLOTS[0]] {
        let mut rig = Rig::new(2);
        rig.play(slot, &src, db_to_gain(0.0), Region::full(&src), EqSettings::default().coeffs(SR));
        let d = rig.latency();
        let out = rig.render(20_000 + d + 10);
        assert!(out[..d * 2].iter().all(|&s| s == 0.0));
        assert_eq!(&out[d * 2..(d + 20_000) * 2], &src.samples[..], "emplacement {slot} : différent de la source");
        assert!(out[(d + 20_000) * 2..].iter().all(|&s| s == 0.0), "rien après la fin");
    }
}

#[test]
fn bit_transparent_on_any_output_pair() {
    let src = noise(5_000, 0.9);
    let mut rig = Rig::new(8);
    rig.send(Command::SetModuleOutput { module: 0, pair: 2 });
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(5_000 + d);
    for c in 0..8 {
        let ch = channel(&out, 8, c);
        match c {
            4 | 5 => assert_eq!(&ch[d..], &channel(&src.samples, 2, c - 4)[..], "canal {c}"),
            _ => assert!(ch.iter().all(|&s| s == 0.0), "fuite sur le canal {c}"),
        }
    }
}

#[test]
fn transparent_after_volume_round_trip() {
    // Volume baissé puis remis à 0 dB : le lissage doit retomber exactement sur 1.
    let src = noise(48_000, 0.5);
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    rig.render(4_000);
    rig.send(Command::SetModuleGain { module: 0, gain: db_to_gain(-12.0) });
    rig.send(Command::SetSlotGain { module: 0, slot: 0, gain: db_to_gain(-6.0) });
    rig.render(4_000);
    rig.send(Command::SetModuleGain { module: 0, gain: 1.0 });
    rig.send(Command::SetSlotGain { module: 0, slot: 0, gain: 1.0 });
    rig.render(8_000);
    let out = rig.render(10_000);
    let d = rig.latency();
    let start = 16_000 - d;
    let expected = &src.samples[start * 2..(start + 10_000) * 2];
    if let Some(i) = (0..out.len()).find(|&i| out[i] != expected[i]) {
        panic!("écart à l'échantillon {i} : {} au lieu de {}", out[i], expected[i]);
    }
}

#[test]
fn null_test_with_gains() {
    let src = noise(10_000, 0.25);
    let (gj, gm) = (db_to_gain(-6.0), db_to_gain(4.5));
    let mut rig = Rig::new(2);
    rig.send(Command::SetModuleGain { module: 0, gain: gm });
    rig.play(0, &src, gj, Region::full(&src), EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(10_000 + d);
    let mut err = 0f64;
    let mut sig = 0f64;
    for (o, s) in out[d * 2..].iter().zip(src.samples.iter()) {
        let expected = (*s as f64) * gj as f64 * gm as f64;
        err += (*o as f64 - expected).powi(2);
        sig += expected * expected;
    }
    let null_db = 10.0 * (err.max(1e-30) / sig).log10();
    assert!(null_db < -135.0, "résidu du null test : {null_db:.1} dB");
}

#[test]
fn summing_is_exact_and_independent() {
    let a = noise(8_000, 0.2);
    let b = sine(440.0, 0.3, 8_000);
    let mut rig = Rig::new(2);
    rig.play(0, &a, 1.0, Region::full(&a), EqCoeffs::flat());
    rig.play(1, &b, 1.0, Region::full(&b), EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(8_000 + d);
    for (i, o) in out[d * 2..].iter().enumerate() {
        assert_eq!(*o, a.samples[i] + b.samples[i]);
    }
}

#[test]
fn harmonic_distortion_is_negligible() {
    // 1 kHz, nombre entier de périodes : seules les erreurs d'arrondi restent.
    let frames = 48_000;
    for (gain_db, eq) in [(0.0, None), (-20.0, None), (6.0, None), (0.0, Some(9.0)), (0.0, Some(-12.0))] {
        let src = sine(1000.0, 0.25, frames);
        let mut rig = Rig::new(2);
        let coeffs = eq.map_or(EqCoeffs::flat(), |g| {
            let mut s = EqSettings::default();
            s.peak1 = Band { on: true, freq: 1000.0, gain_db: g, q: 1.0 };
            s.high_shelf = Band { on: true, freq: 6000.0, gain_db: g / 2.0, q: 0.7 };
            s.coeffs(SR)
        });
        rig.play(0, &src, db_to_gain(gain_db), Region::full(&src), coeffs);
        let d = rig.latency();
        let out = rig.render(frames + d);
        let l = channel(&out, 2, 0);
        // Régime établi (le filtre a fini sa mise en route), 1 s.
        let thd = thd_db(&l[d + 4_800..d + 4_800 + 38_400], 1000.0);
        assert!(thd < -120.0, "THD {thd:.1} dB (gain {gain_db} dB, EQ {eq:?})");
    }
}

// ------------------------------------------------------------------ clics

const CLICK: f32 = 0.02;

fn assert_no_click(out: &[f32], channels: usize, what: &str) {
    assert_finite(out);
    for c in 0..channels.min(2) {
        let d2 = max_d2(&channel(out, channels, c));
        assert!(d2 < CLICK, "{what} : clic (dérivée seconde {d2:.4})");
    }
}

#[test]
fn click_detector_sees_a_hard_cut() {
    // Contrôle du critère : une coupure franche du sinus est bien détectée.
    let mut x = sine_vec(100.0, 0.9, 4_800, PI / 2.0);
    x[1_920..].fill(0.0); // coupure sur une crête
    assert!(max_d2(&x) > 0.5);
    assert!(max_d2(&sine_vec(100.0, 0.9, 4_800, 0.0)) < 0.001);
}

#[test]
fn no_click_on_start_and_stop() {
    let src = sine(100.0, 0.9, 48_000);
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(6_120); // 12,75 périodes : arrêt sur une crête
    rig.send(Command::Stop { module: 0, slot: 0 });
    out.extend(rig.render(4_000));
    assert_no_click(&out, 2, "arrêt manuel");
    assert!(out[out.len() - 2_000..].iter().all(|&s| s == 0.0), "le son s'est éteint");
}

#[test]
fn no_click_on_seek() {
    // Saut d'une crête positive à une crête négative : le micro-fondu doit l'adoucir.
    let src = sine(100.0, 0.9, 96_000);
    let mut rig = Rig::new(2);
    rig.play(PLAYLIST_SLOTS[0], &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(6_120);
    rig.send(Command::Seek { module: 0, slot: PLAYLIST_SLOTS[0], frame: 50_000 + 360 });
    out.extend(rig.render(4_000));
    // Deux sauts rapprochés (glisser rapide sur la barre de lecture).
    rig.send(Command::Seek { module: 0, slot: PLAYLIST_SLOTS[0], frame: 10_000 });
    out.extend(rig.render(100));
    rig.send(Command::Seek { module: 0, slot: PLAYLIST_SLOTS[0], frame: 80_120 });
    out.extend(rig.render(4_000));
    assert_no_click(&out, 2, "déplacement dans le morceau");
}

#[test]
fn seek_is_bit_transparent_after_the_micro_fade() {
    let src = noise(60_000, 0.9);
    let mut rig = Rig::new(2);
    rig.play(PLAYLIST_SLOTS[0], &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let d = rig.latency();
    rig.render(1_000);
    rig.send(Command::Seek { module: 0, slot: PLAYLIST_SLOTS[0], frame: 30_000 });
    let out = rig.render(6_000);
    // Après la latence du limiteur et les deux rampes (sortie puis entrée), la lecture
    // reprend à l'image près, sans aucune altération.
    let ramp = (SR as f32 * 0.010) as usize;
    let k = d + 2 * ramp + 10;
    let played = k - d - ramp; // images jouées depuis la cible
    assert_eq!(&out[k * 2..(k + 2_000) * 2], &src.samples[(30_000 + played) * 2..(30_000 + played + 2_000) * 2]);
}

#[test]
fn no_click_on_pause_and_resume() {
    let src = sine(100.0, 0.9, 96_000);
    let slot = PLAYLIST_SLOTS[0];
    let mut rig = Rig::new(2);
    rig.play(slot, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(6_120);
    rig.send(Command::SetPaused { module: 0, slot, paused: true });
    out.extend(rig.render(3_000));
    rig.send(Command::SetPaused { module: 0, slot, paused: false });
    out.extend(rig.render(3_000));
    assert_no_click(&out, 2, "pause puis reprise");
}

#[test]
fn no_click_when_start_or_end_marker_is_mid_wave() {
    let src = sine(100.0, 0.9, 48_000);
    // Début sur une crête, fin sur une crête, sans fondus réglés.
    let region = Region { start: 120, end: 24_120, looping: None, fade_in: 0, fade_out: 0 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let out = rig.render(30_000);
    assert_no_click(&out, 2, "points de début et de fin");
    assert!(matches!(rig.events.try_recv(), Ok(Event::Ended { .. })));
}

#[test]
fn no_click_on_retrigger() {
    let src = sine(100.0, 0.9, 48_000);
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(6_120);
    // Relance du même jingle depuis le début alors qu'il est sur une crête.
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    out.extend(rig.render(6_000));
    // Arrêt puis relance immédiate pendant le fondu.
    rig.send(Command::Stop { module: 0, slot: 0 });
    out.extend(rig.render(100));
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    out.extend(rig.render(6_000));
    // Relance avec un égaliseur différent : l'ancien son garde le sien pendant son fondu.
    let mut eq = EqSettings::default();
    eq.low_cut = Band { on: true, freq: 300.0, gain_db: 0.0, q: 0.7 };
    rig.play(0, &src, 1.0, Region::full(&src), eq.coeffs(SR));
    out.extend(rig.render(6_000));
    assert_no_click(&out, 2, "relance");
}

#[test]
fn seamless_loop_is_transparent() {
    // Boucle d'un nombre entier de périodes : la sortie est le sinus continu.
    let src = sine(100.0, 0.8, 48_000);
    let region = Region { start: 0, end: 48_000, looping: Some((4_800, 9_600)), fade_in: 0, fade_out: 0 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(30_000 + d);
    let l = channel(&out, 2, 0);
    let reference = sine_vec(100.0, 0.8, 30_000, 0.0);
    let err = l[d..].iter().zip(&reference).fold(0f32, |a, (x, y)| a.max((x - y).abs()));
    assert!(err < 1e-4, "écart à la boucle parfaite : {err}");
    assert!(rig.events.try_recv().is_err(), "la boucle ne se termine jamais");
}

/// Niveau crête de la voie gauche sur la fenêtre `[a, b)` (en images).
fn level(out: &[f32], a: usize, b: usize) -> f32 {
    peak(&channel(out, 2, 0)[a..b])
}

#[test]
fn looping_jingle_fades_in_once_then_holds() {
    // Fondu d'entrée de 0,5 s sur une boucle de 0,1 s : il ne se rejoue pas à chaque tour.
    let src = buffer(vec![0.5; 48_000], vec![0.5; 48_000]);
    let region = Region { start: 0, end: 48_000, looping: Some((4_800, 9_600)), fade_in: 24_000, fade_out: 0 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(60_000 + d);
    assert!(level(&out, d, d + 100) < 0.01, "départ en fondu");
    assert!((level(&out, d + 11_900, d + 12_000) - 0.25).abs() < 0.01, "à mi-fondu, moitié du niveau");
    let after = channel(&out, 2, 0)[d + 24_100..].to_vec();
    assert!(after.iter().all(|&x| (x - 0.5).abs() < 1e-6), "après le fondu, niveau plein à chaque tour");
    assert_no_click(&out, 2, "fondu d'entrée en boucle");
}

#[test]
fn looping_jingle_stops_with_its_whole_fade_out() {
    // Fondu de sortie de 2 s : en boucle, c'est le fondu de l'arrêt, appliqué en entier.
    let src = sine(100.0, 0.8, 48_000);
    let region = Region { start: 0, end: 48_000, looping: Some((4_800, 9_600)), fade_in: 0, fade_out: 96_000 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    rig.render(10_000);
    rig.send(Command::Stop { module: 0, slot: 0 });
    let out = rig.render(110_000);
    let d = rig.latency();
    assert!(level(&out, d + 70_000, d + 72_000) > 0.1, "encore audible à 1,5 s");
    assert!(level(&out, d + 97_000, 110_000) == 0.0, "éteint après 2 s");
    assert_no_click(&out, 2, "arrêt d'une boucle en fondu long");

    // Échap (arrêt d'urgence) : borné à 1 s même avec ce long fondu.
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    rig.render(10_000);
    rig.send(Command::StopJingles);
    let out = rig.render(60_000);
    assert!(level(&out, d + 48_500, 60_000) == 0.0, "Échap : éteint en 1 s au plus");
    assert_no_click(&out, 2, "arrêt d'urgence");
}

#[test]
fn one_shot_jingle_fades_out_at_its_end_and_stops_within_a_second() {
    let src = buffer(vec![0.5; 48_000], vec![0.5; 48_000]);
    let region = Region { start: 0, end: 48_000, looping: None, fade_in: 0, fade_out: 24_000 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(50_000 + d);
    assert!((level(&out, d + 10_000, d + 20_000) - 0.5).abs() < 1e-6, "plein niveau avant le fondu");
    assert!((level(&out, d + 35_990, d + 36_010) - 0.25).abs() < 0.01, "mi-fondu de fin");
    assert!(level(&out, d + 48_000, 50_000 + d) == 0.0, "fini à la fin du fichier");
    // Arrêt manuel d'un son joué une fois : fondu de sortie borné à 1 s (ici 0,5 s, pris en entier).
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    rig.render(1_000);
    rig.send(Command::Stop { module: 0, slot: 0 });
    let out = rig.render(30_000);
    assert!(level(&out, d + 10_000, d + 11_000) > 0.2, "fondu en cours à 0,2 s");
    assert!(level(&out, d + 24_100, 30_000) == 0.0, "éteint après son fondu");
}

#[test]
fn imperfect_loop_points_do_not_click() {
    let src = sine(100.0, 0.9, 48_000);
    // Points de boucle placés n'importe où dans la forme d'onde.
    for (ls, le) in [(4_830, 9_517), (0, 7_211), (1_000, 48_000), (0, 48_000)] {
        let region = Region { start: 0, end: 48_000, looping: Some((ls, le)), fade_in: 0, fade_out: 0 };
        let mut rig = Rig::new(2);
        rig.play(0, &src, 1.0, region, EqCoeffs::flat());
        let out = rig.render(60_000);
        if (ls, le) == (0, 48_000) {
            // Fichier entier en boucle : rien à fondre, le raccord est celui du fichier
            // (ici un sinus entier, donc continu).
            assert_no_click(&out, 2, "boucle sur tout le fichier");
        } else {
            assert_no_click(&out, 2, &format!("boucle {ls}..{le}"));
        }
    }
}

#[test]
fn stop_while_looping_to_file_end_fades_smoothly() {
    // Boucle jusqu'à la fin du fichier, arrêt juste avant la fin : le fondu
    // continue dans la boucle au lieu de buter sur la fin du fichier.
    let src = sine(100.0, 0.9, 48_000);
    let region = Region { start: 0, end: 48_000, looping: Some((24_000, 48_000)), fade_in: 0, fade_out: 0 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let mut out = rig.render(47_900);
    rig.send(Command::Stop { module: 0, slot: 0 });
    out.extend(rig.render(2_000));
    assert_no_click(&out, 2, "arrêt en boucle");
}

#[test]
fn fades_have_no_steps() {
    let src = sine(100.0, 0.9, 48_000);
    let region = Region { start: 0, end: 48_000, looping: None, fade_in: 4_800, fade_out: 9_600 };
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let out = rig.render(50_000);
    assert_no_click(&out, 2, "fondus");
    // Arrêt manuel au milieu du fondu de sortie : repart du niveau courant.
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, region, EqCoeffs::flat());
    let mut out = rig.render(42_000);
    rig.send(Command::Stop { module: 0, slot: 0 });
    out.extend(rig.render(12_000));
    assert_no_click(&out, 2, "arrêt pendant le fondu de sortie");
}

#[test]
fn no_click_on_volume_changes() {
    let src = sine(100.0, 0.9, 96_000);
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = Vec::new();
    for (i, db) in [-40.0, 0.0, -60.0, 6.0, -3.0, -80.0, 0.0].iter().enumerate() {
        out.extend(rig.render(3_000));
        if i % 2 == 0 {
            rig.send(Command::SetModuleGain { module: 0, gain: db_to_gain(*db) });
        } else {
            rig.send(Command::SetSlotGain { module: 0, slot: 0, gain: db_to_gain(*db) });
        }
    }
    out.extend(rig.render(3_000));
    assert_no_click(&out, 2, "changements de volume");
}

#[test]
fn no_click_on_eq_changes() {
    // Signal avec composante continue : un coupe-bas qu'on allume ou éteint
    // ferait sauter le niveau de toute la composante.
    let frames = 96_000;
    let l: Vec<f32> = sine_vec(100.0, 0.3, frames, 0.0).iter().map(|s| s + 0.5).collect();
    let src = buffer(l.clone(), l);
    let mut low_cut = EqSettings::default();
    low_cut.low_cut = Band { on: true, freq: 120.0, gain_db: 0.0, q: 0.7 };
    low_cut.low_cut_slope = CutSlope::Db24;
    let mut boost = EqSettings::default();
    boost.peak1 = Band { on: true, freq: 100.0, gain_db: 12.0, q: 4.0 };

    for module_eq in [false, true] {
        let mut rig = Rig::new(2);
        rig.send(Command::SetModuleGain { module: 0, gain: db_to_gain(-12.0) });
        // Fondu d'entrée : la composante continue du fichier ne doit pas compter comme clic.
        let region = Region { fade_in: 4_800, ..Region::full(&src) };
        rig.play(0, &src, 1.0, region, EqCoeffs::flat());
        let mut out = rig.render(6_000);
        let set = |rig: &Rig, eq: EqCoeffs| {
            rig.send(if module_eq { Command::SetModuleEq { module: 0, eq } } else { Command::SetSlotEq { module: 0, slot: 0, eq } })
        };
        for eq in [low_cut, EqSettings::default(), boost, low_cut] {
            set(&rig, eq.coeffs(SR));
            out.extend(rig.render(6_000));
        }
        // Réglage « tiré à la souris » : un nouveau gain à chaque bloc.
        for i in 0..60 {
            let mut e = boost;
            e.peak1.gain_db = if i % 2 == 0 { 18.0 } else { -18.0 };
            e.peak1.freq = 80.0 + i as f32 * 7.0;
            set(&rig, e.coeffs(SR));
            out.extend(rig.render(256));
        }
        set(&rig, EqSettings::default().coeffs(SR));
        out.extend(rig.render(6_000));
        assert_no_click(&out, 2, &format!("changements d'EQ (module : {module_eq})"));
    }
}

#[test]
fn no_click_when_output_pair_changes() {
    let src = sine(100.0, 0.9, 48_000);
    let mut rig = Rig::new(4);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(6_120);
    rig.send(Command::SetModuleOutput { module: 0, pair: 1 });
    out.extend(rig.render(6_000));
    for c in 0..4 {
        let d2 = max_d2(&channel(&out, 4, c));
        assert!(d2 < CLICK, "sortie {c} : clic au changement de paire ({d2:.4})");
    }
    let tail = &out[out.len() - 4 * 2_000..];
    assert!(tail.chunks_exact(4).all(|f| f[0] == 0.0 && f[1] == 0.0), "l'ancienne paire est éteinte");
}

#[test]
fn gapless_playlist_is_sample_continuous() {
    // Un sinus coupé en deux fichiers : l'enchaînement redonne le sinus continu.
    let whole = sine_vec(100.0, 0.8, 20_000, 0.0);
    let a = buffer(whole[..7_777].to_vec(), whole[..7_777].to_vec());
    let b = buffer(whole[7_777..].to_vec(), whole[7_777..].to_vec());
    let mut rig = Rig::new(2);
    rig.play(PLAYLIST_SLOTS[0], &a, 1.0, Region::full(&a), EqCoeffs::flat());
    rig.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: b, gain: 1.0, id: 9, crossfade: 0 }) });
    let d = rig.latency();
    let out = rig.render(20_000 + d);
    assert_eq!(channel(&out, 2, 0)[d..], whole[..]);
}

#[test]
fn crossfade_is_smooth_and_keeps_level() {
    // Deux morceaux de même nature (sinus en phase) : le fondu enchaîné linéaire
    // garde un niveau constant et ne fait aucun clic.
    let a = sine(100.0, 0.5, 48_000);
    let b = sine(100.0, 0.5, 48_000);
    let mut rig = Rig::new(2);
    rig.play(PLAYLIST_SLOTS[0], &a, 1.0, Region::full(&a), EqCoeffs::flat());
    rig.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: b, gain: 1.0, id: 9, crossfade: 9_600 }) });
    let out = rig.render(90_000);
    assert_no_click(&out, 2, "fondu enchaîné");
    assert!(peak(&out) < 0.55, "pas de bosse de niveau : {}", peak(&out));
}

// --------------------------------------------- robustesse et saturation

#[test]
fn degenerate_regions_never_hang_or_panic() {
    let src = sine(100.0, 0.5, 4_800);
    let empty = buffer(Vec::new(), Vec::new());
    let one = buffer(vec![0.5], vec![0.5]);
    let regions = [
        Region { start: 0, end: 0, looping: None, fade_in: 0, fade_out: 0 },
        Region { start: 3_000, end: 1_000, looping: None, fade_in: 0, fade_out: 0 },
        Region { start: 0, end: 4_800, looping: Some((2_000, 2_000)), fade_in: 0, fade_out: 0 },
        Region { start: 0, end: 4_800, looping: Some((3_000, 1_000)), fade_in: 0, fade_out: 0 },
        Region { start: 0, end: 4_800, looping: Some((2_000, 2_001)), fade_in: 0, fade_out: 0 },
        Region { start: 4_000, end: 4_800, looping: Some((100, 200)), fade_in: 0, fade_out: 0 },
        Region { start: 0, end: 999_999, looping: Some((0, 999_999)), fade_in: 999_999, fade_out: 999_999 },
        Region { start: usize::MAX, end: usize::MAX, looping: Some((usize::MAX - 1, usize::MAX)), fade_in: usize::MAX, fade_out: 0 },
    ];
    for buf in [&src, &empty, &one] {
        for region in regions {
            let mut rig = Rig::new(2);
            rig.play(0, buf, 1.0, region, EqCoeffs::flat());
            let t = Instant::now();
            let out = rig.render(10_000);
            assert!(t.elapsed() < Duration::from_secs(5));
            assert_finite(&out);
            assert!(peak(&out) <= 1.0);
            // Changement de zone en cours de lecture, puis arrêt.
            rig.send(Command::SetRegion { module: 0, slot: 0, region: regions[3] });
            rig.render(1_000);
            rig.send(Command::Stop { module: 0, slot: 0 });
            rig.render(1_000);
        }
    }
}

#[test]
fn empty_tracks_in_playlist_do_not_loop_forever() {
    let empty = buffer(Vec::new(), Vec::new());
    let a = sine(100.0, 0.5, 1_000);
    let mut rig = Rig::new(2);
    rig.play(PLAYLIST_SLOTS[0], &empty, 1.0, Region::full(&empty), EqCoeffs::flat());
    rig.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: empty.clone(), gain: 1.0, id: 5, crossfade: 0 }) });
    rig.render(512);
    rig.play(PLAYLIST_SLOTS[1], &a, 1.0, Region::full(&a), EqCoeffs::flat());
    rig.send(Command::QueueNext { module: 0, next: Some(Queued { buffer: empty, gain: 1.0, id: 6, crossfade: 48_000 }) });
    let out = rig.render(5_000);
    assert_finite(&out);
}

#[test]
fn moving_end_marker_before_playhead_fades_out() {
    let src = sine(100.0, 0.9, 48_000);
    let mut rig = Rig::new(2);
    rig.play(0, &src, 1.0, Region::full(&src), EqCoeffs::flat());
    let mut out = rig.render(12_120);
    rig.send(Command::SetRegion { module: 0, slot: 0, region: Region { start: 0, end: 5_000, looping: None, fade_in: 0, fade_out: 0 } });
    out.extend(rig.render(3_000));
    assert_no_click(&out, 2, "fin déplacée avant la tête de lecture");
    assert!(out[out.len() - 1_000..].iter().all(|&s| s == 0.0));
}

#[test]
fn many_loud_sounds_never_clip_the_output() {
    // Tous les emplacements de tous les modules à +10 dB, modules à +10 dB, sur la même paire.
    let src = noise(24_000, 1.0);
    let tone = sine(60.0, 1.0, 24_000);
    let mut rig = Rig::new(2);
    for m in 0..MAX_MODULES {
        rig.send(Command::SetModuleGain { module: m, gain: db_to_gain(10.0) });
        for s in 0..SLOTS {
            let buf = if s % 2 == 0 { &src } else { &tone };
            rig.send(Command::Play { module: m, slot: s, id: 1, buffer: buf.clone(), gain: db_to_gain(10.0), region: Region::full(buf), eq: EqCoeffs::flat() });
        }
    }
    let out = rig.render(30_000);
    assert_finite(&out);
    let p = peak(&out);
    assert!(p <= 1.0, "crête {p} : écrêtage");
    assert!(p > 0.9, "le limiteur ne doit pas écraser plus que nécessaire : {p}");
}

#[test]
fn extreme_eq_settings_stay_finite_and_bounded() {
    let impulse = {
        let mut v = vec![0f32; 12_000];
        v[0] = 1.0;
        v
    };
    let src_noise = noise(48_000, 1.0);
    let values = [f32::NAN, f32::INFINITY, -f32::INFINITY, 0.0, -5.0, 1e-9, 10.0, 20.0, 19_999.0, 30_000.0, 1e9];
    let gains = [f32::NAN, -1e9, -18.0, 0.0, 18.0, 1e9];
    let qs = [f32::NAN, 0.0, 1e-6, 0.1, 18.0, 1e6];
    for &sr in &[8_000u32, 44_100, 48_000, 96_000, 192_000] {
        for &f in &values {
            for &g in &gains {
                for &q in &qs {
                    let mut eq = EqSettings::default();
                    let band = Band { on: true, freq: f, gain_db: g, q };
                    eq.low_cut = band;
                    eq.low_cut_slope = CutSlope::Db24;
                    eq.low_shelf = band;
                    eq.peak1 = band;
                    eq.peak2 = band;
                    eq.high_shelf = band;
                    let c = eq.coeffs(sr);
                    let r = c.max_pole_radius();
                    assert!(r < 1.0, "filtre instable : f={f} g={g} q={q} sr={sr} (pôle {r})");
                    let mut st = EqState::default();
                    let mut buf: Vec<f32> = impulse.iter().flat_map(|&s| [s, s]).collect();
                    st.process(&c, &mut buf);
                    assert!(buf.iter().all(|s| s.is_finite()), "NaN : f={f} g={g} q={q} sr={sr}");
                    // Réponse impulsionnelle bornée (gain maximal des bandes : +18 dB chacune).
                    assert!(peak(&buf) < 1e5, "explosion : f={f} g={g} q={q} sr={sr}");
                }
            }
        }
    }
    // Bruit pleine échelle à travers l'EQ le plus poussé : gain borné par celui des bandes.
    let mut eq = EqSettings::default();
    for b in [&mut eq.low_shelf, &mut eq.peak1, &mut eq.peak2, &mut eq.high_shelf] {
        *b = Band { on: true, freq: 1000.0, gain_db: 18.0, q: 18.0 };
    }
    let c = eq.coeffs(SR);
    let mut st = EqState::default();
    let mut buf = src_noise.samples.clone();
    st.process(&c, &mut buf);
    assert!(buf.iter().all(|s| s.is_finite()));
    // Une entrée non finie n'empoisonne pas le filtre pour toujours.
    let mut bad = vec![f32::NAN, f32::NAN];
    st.process(&c, &mut bad);
    let mut after = vec![0.25f32; 2_000];
    st.process(&c, &mut after);
    assert!(after.iter().all(|s| s.is_finite()), "le filtre repart après un NaN");
}

#[test]
fn eq_processor_crossfade_reaches_new_setting() {
    let mut boost = EqSettings::default();
    boost.peak1 = Band { on: true, freq: 1000.0, gain_db: 6.0, q: 1.0 };
    let mut p = EqProcessor::new(EqCoeffs::flat(), 960);
    let mut scratch = vec![0f32; 2 * 48_000];
    let mut buf: Vec<f32> = sine_vec(1000.0, 0.25, 48_000, 0.0).iter().flat_map(|&s| [s, s]).collect();
    p.set(boost.coeffs(SR));
    for chunk in buf.chunks_mut(512) {
        let n = chunk.len();
        p.process(chunk, &mut scratch[..n]);
    }
    let gain_db = 20.0 * (peak(&buf[40_000..]) as f64 / 0.25).log10();
    assert!((gain_db - 6.0).abs() < 0.1, "gain final {gain_db}");
    p.set(EqCoeffs::flat());
    for chunk in buf.chunks_mut(512) {
        let n = chunk.len();
        p.process(chunk, &mut scratch[..n]);
    }
    assert!(p.is_bypassed(), "retour au contournement exact");
}

// ------------------------------------------------------------- temps réel

#[test]
fn process_never_allocates() {
    let src = sine(100.0, 0.5, 48_000);
    let short = sine(100.0, 0.5, 300);
    let mut eq = EqSettings::default();
    eq.peak1 = Band { on: true, freq: 500.0, gain_db: 6.0, q: 2.0 };
    let mut rig = Rig::new(8);
    // Premier passage pour que tout soit en place, puis mesure.
    rig.render(512);
    let cmds = |rig: &Rig, round: usize| {
        for m in 0..MAX_MODULES {
            rig.send(Command::SetModuleOutput { module: m, pair: round % 4 });
            rig.send(Command::SetModuleGain { module: m, gain: db_to_gain(-(round as f32)) });
            rig.send(Command::SetModuleEq { module: m, eq: if round % 2 == 0 { eq.coeffs(SR) } else { EqCoeffs::flat() } });
            for s in 0..4 {
                let region = Region { start: 10, end: 40_000, looping: Some((100, 900)), fade_in: 50, fade_out: 50 };
                rig.send(Command::Play { module: m, slot: s, id: 1, buffer: src.clone(), gain: 0.5, region, eq: eq.coeffs(SR) });
            }
            rig.send(Command::SetSlotEq { module: m, slot: 1, eq: EqCoeffs::flat() });
            rig.send(Command::SetRegion { module: m, slot: 2, region: Region::full(&src) });
            rig.send(Command::Stop { module: m, slot: 3 });
            rig.send(Command::Seek { module: m, slot: 0, frame: 20_000 + round * 100 });
            rig.send(Command::SetPaused { module: m, slot: PLAYLIST_SLOTS[0], paused: round.is_multiple_of(2) });
            rig.send(Command::Play { module: m, slot: PLAYLIST_SLOTS[0], id: 2, buffer: short.clone(), gain: 1.0, region: Region::full(&short), eq: EqCoeffs::flat() });
            rig.send(Command::QueueNext { module: m, next: Some(Queued { buffer: short.clone(), gain: 1.0, id: 3, crossfade: 100 }) });
        }
    };
    let mut out = vec![0f32; 8 * 1024];
    for round in 0..6 {
        cmds(&rig, round);
        ARMED.with(|a| a.set(true));
        for _ in 0..8 {
            rig.mixer.process(&mut out);
        }
        rig.send(Command::StopAll);
        rig.mixer.process(&mut out);
        ARMED.with(|a| a.set(false));
        // Le thread de libération vide la corbeille hors du callback.
        while rig._garbage.try_recv().is_ok() {}
        while rig.events.try_recv().is_ok() {}
    }
    let n = ALLOCS.with(|c| c.get());
    assert_eq!(n, 0, "{n} allocations ou libérations dans le callback");
}

#[test]
fn stale_play_commands_are_dropped_when_device_reopens() {
    // Clics sur des jingles pendant que le périphérique était fermé : à la réouverture
    // ils ne doivent pas tous partir d'un coup.
    let (tx, rx) = bounded(4096);
    let (etx, _erx) = bounded(64);
    let (gtx, _grx) = bounded(64);
    let src = sine(100.0, 1.0, 48_000);
    for s in 0..12 {
        tx.send(Command::Play { module: 0, slot: s, id: 1, buffer: src.clone(), gain: 1.0, region: Region::full(&src), eq: EqCoeffs::flat() }).unwrap();
    }
    tx.send(Command::SetModuleGain { module: 0, gain: 0.5 }).unwrap();
    let mut mixer = Mixer::new(2, SR, rx, etx, gtx, Arc::new(Telemetry::new()));
    let mut out = vec![0f32; 2 * 1024];
    mixer.process(&mut out);
    assert!(out.iter().all(|&s| s == 0.0));
    // Les réglages, eux, sont gardés.
    tx.send(Command::Play { module: 0, slot: 0, id: 2, buffer: src.clone(), gain: 1.0, region: Region::full(&src), eq: EqCoeffs::flat() }).unwrap();
    for _ in 0..10 {
        mixer.process(&mut out);
    }
    assert!(peak(&out) > 0.45 && peak(&out) < 0.51, "volume du module conservé : {}", peak(&out));
}

// ------------------------------------------------------ décodage et rééchantillonnage

fn write_wav(path: &std::path::Path, rate: u32, channels: u16, samples: &[i16]) {
    let mut b = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    b.extend_from_slice(&(channels * 2).to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

fn temp_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("jinglestar-tests-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn wav_16_bit_decodes_bit_exact() {
    let dir = temp_dir();
    let path = dir.join("stereo.wav");
    let samples: Vec<i16> = (0..20_000).map(|i| ((i * 7919) % 65_536 - 32_768) as i16).collect();
    write_wav(&path, SR, 2, &samples);
    let buf = decode_file(&path, SR).unwrap();
    assert_eq!(buf.frames, 10_000);
    for (d, s) in buf.samples.iter().zip(&samples) {
        assert_eq!(*d, *s as f32 / 32_768.0);
    }
    // Lecture complète par le moteur : identique au fichier.
    let buf = Arc::new(buf);
    let mut rig = Rig::new(2);
    rig.play(0, &buf, 1.0, Region::full(&buf), EqCoeffs::flat());
    let d = rig.latency();
    let out = rig.render(10_000 + d);
    assert_eq!(&out[d * 2..], &buf.samples[..]);
    // Mono : le même signal sur les deux canaux, sans atténuation.
    let path = dir.join("mono.wav");
    write_wav(&path, SR, 1, &samples[..1_000]);
    let buf = decode_file(&path, SR).unwrap();
    for (i, f) in buf.samples.chunks_exact(2).enumerate() {
        assert_eq!(f[0], samples[i] as f32 / 32_768.0);
        assert_eq!(f[0], f[1]);
    }
}

#[test]
fn broken_files_fail_fast_without_hanging() {
    let dir = temp_dir();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("vide.wav", Vec::new()),
        ("bruit.wav", (0..10_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect()),
        ("faux.mp3", b"ID3\x04\x00\x00\x00\x00\x00\x00garbage garbage garbage".to_vec()),
        ("entete.wav", b"RIFF\x24\x00\x00\x00WAVEfmt ".to_vec()),
    ];
    for (name, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let t = Instant::now();
        let res = decode_file(&path, SR);
        assert!(t.elapsed() < Duration::from_secs(5), "{name} : trop long");
        assert!(res.is_err(), "{name} : devrait être refusé");
    }
    // En-tête valide sans aucune donnée.
    let path = dir.join("sans-donnees.wav");
    write_wav(&path, SR, 2, &[]);
    assert!(decode_file(&path, SR).is_err());
    // Fichier tronqué : l'en-tête annonce plus de données qu'il n'y en a.
    let path = dir.join("tronque.wav");
    let samples: Vec<i16> = (0..4_000).map(|i| (i * 3) as i16).collect();
    write_wav(&path, SR, 2, &samples);
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 3_001]).unwrap();
    let t = Instant::now();
    if let Ok(buf) = decode_file(&path, SR) {
        assert!(buf.frames <= 2_000 && buf.samples.iter().all(|s| s.is_finite()));
    }
    assert!(t.elapsed() < Duration::from_secs(5));
    // Fichier introuvable.
    assert!(decode_file(&dir.join("absent.wav"), SR).is_err());
}

#[test]
fn resampling_is_clean_and_aligned() {
    let dir = temp_dir();
    for (from, freq) in [(44_100u32, 1_000.0), (96_000, 1_000.0), (22_050, 440.0)] {
        let frames = from as usize * 2;
        let samples: Vec<i16> = (0..frames)
            .flat_map(|i| {
                let v = ((2.0 * PI * freq * i as f64 / from as f64).sin() * 0.5 * 32_767.0).round() as i16;
                [v, v]
            })
            .collect();
        let path = dir.join(format!("sinus-{from}.wav"));
        write_wav(&path, from, 2, &samples);
        let buf = decode_file(&path, SR).unwrap();
        assert_eq!(buf.frames, frames * SR as usize / from as usize, "durée conservée ({from} Hz)");
        let l = channel(&buf.samples, 2, 0);
        let mid = &l[SR as usize / 4..SR as usize * 7 / 4];
        let thdn = thd_n_db(mid, freq, SR as f64);
        assert!(thdn < -80.0, "THD+N après {from} → {SR} : {thdn:.1} dB");
        // Pas de décalage temporel : en phase avec le sinus idéal au nouveau taux.
        let offset = SR as usize / 4;
        let err = mid.iter().enumerate().fold(0f64, |a, (i, &s)| {
            let ideal = (2.0 * PI * freq * (i + offset) as f64 / SR as f64).sin() * 0.5;
            a.max((s as f64 - ideal).abs())
        });
        assert!(err < 0.01, "décalage après {from} → {SR} : {err}");
        assert!(peak(&l) < 0.52, "pas de dépassement au rééchantillonnage");
    }
    // Repliement : une fréquence au-dessus de la nouvelle limite de Nyquist disparaît.
    let from = 96_000u32;
    let samples: Vec<i16> = (0..from as usize)
        .flat_map(|i| {
            let v = ((2.0 * PI * 30_000.0 * i as f64 / from as f64).sin() * 0.5 * 32_767.0) as i16;
            [v, v]
        })
        .collect();
    let path = dir.join("aigu.wav");
    write_wav(&path, from, 2, &samples);
    let buf = decode_file(&path, SR).unwrap();
    let l = channel(&buf.samples, 2, 0);
    let level = 20.0 * (peak(&l[4_800..43_200]) as f64 / 0.5).log10();
    assert!(level < -60.0, "repliement à {level:.1} dB");
    // Fichiers minuscules.
    for n in [1usize, 2, 7, 1023, 1025] {
        let path = dir.join(format!("court-{n}.wav"));
        write_wav(&path, 44_100, 2, &vec![1_000i16; n * 2]);
        let buf = decode_file(&path, SR).unwrap();
        assert_eq!(buf.frames, n * 48_000 / 44_100);
    }
}
