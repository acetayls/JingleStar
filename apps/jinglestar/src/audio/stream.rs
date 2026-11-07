//! Sons longs lus depuis le disque.
//!
//! Un son de plus de `STREAM_MIN_SECS` n'est pas gardé en entier en mémoire : il est écrit,
//! déjà décodé et rééchantillonné, dans un fichier temporaire, découpé en blocs. Seuls restent
//! en mémoire le début du son, les points de départ demandés (`set_pins`) et, pendant la
//! lecture, quelques secondes autour de chaque tête de lecture. Un fil de fond (`streamer`)
//! charge les blocs avant qu'on en ait besoin et rend ceux qui ne servent plus.
//!
//! Le fil audio ne fait que lire : jamais d'attente du disque, de verrou ni de libération.
//! Un bloc absent au moment de le jouer donne du silence, compté dans `take_underrun_frames`.
//! Un bloc retiré n'est libéré qu'une seconde plus tard, quand plus aucun callback audio
//! ne peut encore le lire.

use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

/// Taille d'un bloc, en images stéréo (0,68 s à 48 kHz, 256 Ko).
pub const BLOCK_FRAMES: usize = 32_768;
const BLOCK_SAMPLES: usize = BLOCK_FRAMES * 2;
/// Durée au-delà de laquelle un son est lu depuis le disque.
pub const STREAM_MIN_SECS: f64 = 30.0;
/// Durée gardée en mémoire au début du son et à chaque point de départ.
const HEAD_SECS: f64 = 10.0;
/// Avance de lecture gardée en mémoire devant chaque tête de lecture.
const AHEAD_SECS: f64 = 5.0;
/// Lectures simultanées suivies par son (au-delà, la lecture se fait sans avance).
const READERS: usize = 16;
/// Délai avant de libérer un bloc retiré (bien plus long qu'un callback audio).
const FREE_DELAY: Duration = Duration::from_secs(1);
/// Une lecture qui ne donne plus signe de vie depuis ce délai est considérée comme finie.
const READER_TIMEOUT: Duration = Duration::from_secs(2);
/// Pas d'attente du fil de fond entre deux passages.
const TICK: Duration = Duration::from_millis(5);
/// Blocs de points de départ chargés au plus par passage, pour revenir vite aux lectures.
const PIN_LOADS_PER_TICK: usize = 8;
const NONE: u64 = u64::MAX;

type Block = [f32; BLOCK_SAMPLES];

static UNDERRUN_FRAMES: AtomicU64 = AtomicU64::new(0);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<Vec<Weak<Stream>>>> = OnceLock::new();

/// Images jouées en silence faute de bloc chargé à temps, depuis le dernier appel.
pub fn take_underrun_frames() -> u64 {
    UNDERRUN_FRAMES.swap(0, Ordering::Relaxed)
}

/// Une tête de lecture, publiée par le fil audio pour que le fil de fond charge ce qui suit.
struct Reader {
    /// Jeton du propriétaire, 0 si libre.
    owner: AtomicU64,
    pos: AtomicU64,
    /// Début de boucle ou cible d'un déplacement, `NONE` sinon.
    extra: AtomicU64,
    /// Fin de boucle (le fondu de boucle y lit quelques images), `NONE` sinon.
    loop_end: AtomicU64,
    /// Augmente à chaque publication : signe de vie.
    beat: AtomicU64,
}

/// Lecture en cours d'un son : emplacement de tête et jeton.
pub type ReaderSlot = Option<(usize, u64)>;

pub struct Stream {
    file: Option<File>,
    frames: usize,
    rate: u32,
    blocks: Box<[AtomicPtr<Block>]>,
    resident: AtomicUsize,
    readers: [Reader; READERS],
    /// Points de départ à garder prêts (images), en plus du début du son.
    pins: Mutex<Vec<usize>>,
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stream({} images, {} blocs en mémoire)", self.frames, self.resident.load(Ordering::Relaxed))
    }
}

impl Stream {
    /// Écrit le son entrelacé `samples` dans un fichier temporaire et garde en mémoire
    /// les blocs du début. Le fichier disparaît à la fermeture (ou au plantage) du programme.
    pub fn create(samples: &[f32], rate: u32) -> std::io::Result<Arc<Stream>> {
        let file = temp_file()?;
        // Écriture par blocs : pas de seconde copie du son entier en octets.
        let mut bytes = Vec::with_capacity(BLOCK_SAMPLES * 4);
        for (i, chunk) in samples.chunks(BLOCK_SAMPLES).enumerate() {
            bytes.clear();
            chunk.iter().for_each(|s| bytes.extend_from_slice(&s.to_ne_bytes()));
            write_at(&file, &bytes, (i * BLOCK_SAMPLES * 4) as u64)?;
        }
        let frames = samples.len() / 2;
        let count = frames.div_ceil(BLOCK_FRAMES).max(1);
        let stream = Arc::new(Stream {
            file: Some(file),
            frames,
            rate,
            blocks: (0..count).map(|_| AtomicPtr::new(std::ptr::null_mut())).collect(),
            resident: AtomicUsize::new(0),
            readers: std::array::from_fn(|_| Reader {
                owner: AtomicU64::new(0),
                pos: AtomicU64::new(NONE),
                extra: AtomicU64::new(NONE),
                loop_end: AtomicU64::new(NONE),
                beat: AtomicU64::new(0),
            }),
            pins: Mutex::new(Vec::new()),
        });
        // Début du son : copié depuis la mémoire, sans relire le disque.
        for b in stream.head_blocks(0) {
            let start = b * BLOCK_SAMPLES;
            let end = (start + BLOCK_SAMPLES).min(samples.len());
            let mut block = new_block();
            block[..end - start].copy_from_slice(&samples[start..end]);
            stream.install(b, block);
        }
        let registry = REGISTRY.get_or_init(|| {
            std::thread::Builder::new().name("audio-streamer".into()).spawn(streamer).expect("thread audio-streamer");
            Mutex::new(Vec::new())
        });
        registry.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::downgrade(&stream));
        Ok(stream)
    }

    /// Mémoire occupée par les blocs chargés.
    pub fn resident_bytes(&self) -> usize {
        self.resident.load(Ordering::Relaxed) * BLOCK_SAMPLES * std::mem::size_of::<f32>()
    }

    /// Points de départ (images) à garder prêts en mémoire : points d'entrée des jingles,
    /// des cues, départ d'un morceau en cours de liste.
    pub fn set_pins(&self, mut pins: Vec<usize>) {
        pins.retain(|&p| p > 0 && p < self.frames);
        pins.sort_unstable();
        pins.dedup();
        *self.pins.lock().unwrap_or_else(|e| e.into_inner()) = pins;
    }

    /// Échantillon stéréo à l'image `p` (fil audio). Bloc pas encore chargé : silence.
    #[inline]
    pub fn frame(&self, p: usize) -> (f32, f32) {
        let ptr = match self.blocks.get(p / BLOCK_FRAMES) {
            Some(b) => b.load(Ordering::Acquire),
            None => return (0.0, 0.0),
        };
        if ptr.is_null() {
            UNDERRUN_FRAMES.fetch_add(1, Ordering::Relaxed);
            return (0.0, 0.0);
        }
        // SAFETY : un bloc publié n'est libéré que `FREE_DELAY` après avoir été retiré,
        // et un callback audio dure bien moins longtemps.
        let block = unsafe { &*ptr };
        let i = (p % BLOCK_FRAMES) * 2;
        (block[i], block[i + 1])
    }

    /// Publie la position d'une lecture (fil audio, sans verrou ni allocation). Renvoie
    /// l'emplacement à repasser au prochain appel ; `None` si toutes les places sont prises.
    #[inline]
    pub fn publish(&self, slot: ReaderSlot, pos: usize, extra: Option<usize>, loop_end: Option<usize>) -> ReaderSlot {
        let (i, token) = match slot {
            Some((i, t)) if self.readers[i].owner.load(Ordering::Relaxed) == t => (i, t),
            _ => {
                let t = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
                let i = (0..READERS).find(|&i| {
                    self.readers[i].owner.compare_exchange(0, t, Ordering::AcqRel, Ordering::Relaxed).is_ok()
                })?;
                (i, t)
            }
        };
        let r = &self.readers[i];
        r.pos.store(pos as u64, Ordering::Relaxed);
        r.extra.store(extra.map_or(NONE, |e| e as u64), Ordering::Relaxed);
        r.loop_end.store(loop_end.map_or(NONE, |e| e as u64), Ordering::Relaxed);
        r.beat.fetch_add(1, Ordering::Release);
        Some((i, token))
    }

    /// Rend la place d'une lecture terminée.
    pub fn release(&self, slot: ReaderSlot) {
        if let Some((i, t)) = slot {
            let _ = self.readers[i].owner.compare_exchange(t, 0, Ordering::AcqRel, Ordering::Relaxed);
        }
    }

    fn secs_to_blocks(&self, secs: f64) -> usize {
        ((secs * self.rate.max(1) as f64) as usize).div_ceil(BLOCK_FRAMES).max(1)
    }

    /// Blocs couvrant `HEAD_SECS` à partir de l'image `from`.
    fn head_blocks(&self, from: usize) -> std::ops::Range<usize> {
        let b = from / BLOCK_FRAMES;
        b..(b + self.secs_to_blocks(HEAD_SECS) + 1).min(self.blocks.len())
    }

    /// Blocs d'une tête de lecture : un bloc avant (fondus de boucle), puis l'avance.
    fn around(&self, at: usize, ahead: usize) -> std::ops::Range<usize> {
        let b = at / BLOCK_FRAMES;
        b.saturating_sub(1)..(b + ahead + 1).min(self.blocks.len())
    }

    fn install(&self, b: usize, block: Box<Block>) {
        let old = self.blocks[b].swap(Box::into_raw(block), Ordering::AcqRel);
        if old.is_null() {
            self.resident.fetch_add(1, Ordering::Relaxed);
        } else {
            // Ne devrait pas arriver (un seul fil charge) : on ne libère pas un bloc peut-être lu.
            retire_later(old);
        }
    }

    fn load(&self, b: usize) -> std::io::Result<()> {
        let Some(file) = self.file.as_ref() else { return Ok(()) };
        let start = b * BLOCK_SAMPLES;
        let len = (self.frames * 2).saturating_sub(start).min(BLOCK_SAMPLES);
        let mut bytes = vec![0u8; len * 4];
        read_at(file, &mut bytes, (start * 4) as u64)?;
        let mut block = new_block();
        for (d, s) in block.iter_mut().zip(bytes.chunks_exact(4)) {
            *d = f32::from_ne_bytes([s[0], s[1], s[2], s[3]]);
        }
        self.install(b, block);
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Plus personne ne lit ce son (dernière référence) : libération immédiate.
        for b in self.blocks.iter() {
            let p = b.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !p.is_null() {
                // SAFETY : pointeur issu de `Box::into_raw`, retiré du tableau.
                drop(unsafe { Box::from_raw(p) });
            }
        }
        // Fermer le fichier le supprime (fichier temporaire).
        self.file.take();
    }
}

fn new_block() -> Box<Block> {
    vec![0f32; BLOCK_SAMPLES].into_boxed_slice().try_into().expect("taille de bloc")
}

/// Blocs retirés en attente de libération.
static RETIRED: Mutex<Vec<(usize, Instant)>> = Mutex::new(Vec::new());

fn retire_later(p: *mut Block) {
    RETIRED.lock().unwrap_or_else(|e| e.into_inner()).push((p as usize, Instant::now()));
}

fn free_retired() {
    let mut list = RETIRED.lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|&(p, at)| {
        if at.elapsed() < FREE_DELAY {
            return true;
        }
        // SAFETY : retiré du tableau depuis `FREE_DELAY`, plus aucun callback ne le lit.
        drop(unsafe { Box::from_raw(p as *mut Block) });
        false
    });
}

/// Suivi d'une tête de lecture par le fil de fond : dernier signe de vie vu.
struct Seen {
    stream: usize,
    reader: usize,
    beat: u64,
    at: Instant,
}

/// Fil de fond : charge les blocs attendus, rend ceux qui ne servent plus.
fn streamer() {
    let mut seen: Vec<Seen> = Vec::new();
    loop {
        std::thread::sleep(TICK);
        let streams: Vec<Arc<Stream>> = {
            let Some(reg) = REGISTRY.get() else { continue };
            let mut reg = reg.lock().unwrap_or_else(|e| e.into_inner());
            reg.retain(|w| w.strong_count() > 0);
            reg.iter().filter_map(Weak::upgrade).collect()
        };
        let mut wanted = Vec::new();
        // Les lectures en cours d'abord, pour tous les sons ; les points de départ ensuite.
        for s in &streams {
            wanted.clear();
            let ahead = s.secs_to_blocks(AHEAD_SECS);
            for (i, r) in s.readers.iter().enumerate() {
                let owner = r.owner.load(Ordering::Acquire);
                if owner == 0 {
                    continue;
                }
                let beat = r.beat.load(Ordering::Acquire);
                let key = (Arc::as_ptr(s) as usize, i);
                match seen.iter_mut().find(|x| (x.stream, x.reader) == key) {
                    Some(x) if x.beat != beat => (x.beat, x.at) = (beat, Instant::now()),
                    Some(x) if x.at.elapsed() > READER_TIMEOUT => {
                        // Lecture disparue sans rendre sa place (sortie audio rouverte…).
                        let _ = r.owner.compare_exchange(owner, 0, Ordering::AcqRel, Ordering::Relaxed);
                        continue;
                    }
                    Some(_) => {}
                    None => seen.push(Seen { stream: key.0, reader: i, beat, at: Instant::now() }),
                }
                let pos = r.pos.load(Ordering::Relaxed);
                if pos != NONE {
                    wanted.extend(s.around(pos as usize, ahead));
                }
                let extra = r.extra.load(Ordering::Relaxed);
                if extra != NONE {
                    wanted.extend(s.around(extra as usize, ahead));
                }
                let end = r.loop_end.load(Ordering::Relaxed);
                if end != NONE {
                    wanted.extend(s.around(end as usize, 1));
                }
            }
            for &b in &wanted {
                if s.blocks[b].load(Ordering::Acquire).is_null() {
                    if let Err(e) = s.load(b) {
                        star_core::log::write(&format!("Lecture disque impossible : {e}"));
                    }
                }
            }
        }
        let mut pin_loads = 0;
        for s in &streams {
            let pins = s.pins.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut keep: Vec<usize> = s.head_blocks(0).collect();
            for p in pins {
                keep.extend(s.head_blocks(p));
            }
            for &b in &keep {
                if pin_loads < PIN_LOADS_PER_TICK && s.blocks[b].load(Ordering::Acquire).is_null() {
                    pin_loads += 1;
                    let _ = s.load(b);
                }
            }
            // Ce qui n'est ni un départ ni autour d'une lecture est rendu.
            let ahead = s.secs_to_blocks(AHEAD_SECS);
            for r in s.readers.iter().filter(|r| r.owner.load(Ordering::Acquire) != 0) {
                for v in [r.pos.load(Ordering::Relaxed), r.extra.load(Ordering::Relaxed)] {
                    if v != NONE {
                        keep.extend(s.around(v as usize, ahead));
                    }
                }
                let end = r.loop_end.load(Ordering::Relaxed);
                if end != NONE {
                    keep.extend(s.around(end as usize, 1));
                }
            }
            for (b, slot) in s.blocks.iter().enumerate() {
                if !keep.contains(&b) && !slot.load(Ordering::Acquire).is_null() {
                    let old = slot.swap(std::ptr::null_mut(), Ordering::AcqRel);
                    if !old.is_null() {
                        s.resident.fetch_sub(1, Ordering::Relaxed);
                        retire_later(old);
                    }
                }
            }
        }
        seen.retain(|x| streams.iter().any(|s| Arc::as_ptr(s) as usize == x.stream));
        drop(streams);
        free_retired();
    }
}

/// Fichier temporaire supprimé automatiquement à sa fermeture.
fn temp_file() -> std::io::Result<File> {
    let dir = directories::ProjectDirs::from("com", "acetayls", "JingleStar")
        .map(|d| d.cache_dir().join("flux"))
        .unwrap_or_else(|| std::env::temp_dir().join("JingleStar-flux"));
    std::fs::create_dir_all(&dir)?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name: PathBuf = dir.join(format!("{}-{}.raw", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
        opts.custom_flags(FILE_FLAG_DELETE_ON_CLOSE);
    }
    let file = opts.open(&name)?;
    // Ailleurs : le nom est retiré tout de suite, le contenu vit jusqu'à la fermeture.
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(&name);
    Ok(file)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_write(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(windows)]
fn read_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_read(buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bloc de l'image `p` chargé. On attend ainsi sans appeler `frame`, qui compterait du
    /// silence dans le compteur global (lu par les tests du contrôleur).
    fn loaded(s: &Stream, p: usize) -> bool {
        !s.blocks[p / BLOCK_FRAMES].load(Ordering::Acquire).is_null()
    }

    fn wait_for(cond: impl Fn() -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(5) {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn streamed_sound_reads_back_identical() {
        // 60 s à 1 kHz : le début (10 s) est en mémoire tout de suite, le reste arrive à la demande.
        let rate = 1000;
        let frames = BLOCK_FRAMES * 4 + 123;
        let samples: Vec<f32> = (0..frames * 2).map(|i| i as f32).collect();
        let s = Stream::create(&samples, rate).unwrap();
        assert_eq!(s.frame(5), (10.0, 11.0));
        let far = BLOCK_FRAMES * 4 + 100;
        let slot = s.publish(None, far, None, None);
        assert!(slot.is_some());
        assert!(wait_for(|| loaded(&s, far)), "bloc lointain chargé");
        assert_eq!(s.frame(far), ((far * 2) as f32, (far * 2 + 1) as f32));
        assert_eq!(s.frame(frames + 10), (0.0, 0.0), "au-delà de la fin : silence");
        s.release(slot);
    }

    #[test]
    fn pins_are_kept_and_unused_blocks_released() {
        let rate = 1000;
        let frames = BLOCK_FRAMES * 6;
        let samples = vec![0.5f32; frames * 2];
        let s = Stream::create(&samples, rate).unwrap();
        let pin = BLOCK_FRAMES * 5 + 7;
        s.set_pins(vec![pin]);
        assert!(wait_for(|| loaded(&s, pin)), "point de départ chargé");
        assert_eq!(s.frame(pin), (0.5, 0.5));
        // Une lecture au milieu fait venir ses blocs, qui repartent quand elle se termine.
        let mid = BLOCK_FRAMES * 3;
        let slot = s.publish(None, mid, None, None);
        assert!(wait_for(|| loaded(&s, mid)));
        s.release(slot);
        s.set_pins(Vec::new());
        assert!(wait_for(|| s.resident.load(Ordering::Relaxed) == s.head_blocks(0).len()), "mémoire rendue");
    }
}
