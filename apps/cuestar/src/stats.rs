//! Charge de la machine pour la barre d'état : % CPU (toutes plateformes), % GPU (Windows).

use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Default)]
pub struct Load {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
}

/// Lance la mesure en tâche de fond (une fois par seconde).
pub fn start() -> Arc<Mutex<Load>> {
    let shared = Arc::new(Mutex::new(Load::default()));
    let out = shared.clone();
    std::thread::Builder::new()
        .name("charge".into())
        .spawn(move || {
            let mut sys = sysinfo::System::new().ok();
            let mut gpu = gpu::Probe::new();
            loop {
                let cpu = sys.as_mut().map(|s| {
                    s.refresh_cpu_usage();
                    s.global_cpu_usage()
                });
                let load = Load { cpu, gpu: gpu.as_mut().and_then(|g| g.sample()) };
                if let Ok(mut l) = out.lock() {
                    *l = load;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .ok();
    shared
}

#[cfg(windows)]
mod gpu {
    //! Compteurs de performance Windows « GPU Engine » (comme le Gestionnaire des tâches) :
    //! utilisation 3D, additionnée par carte graphique ; on garde la carte la plus chargée.
    use std::collections::HashMap;
    use windows_sys::Win32::System::Performance::*;

    pub struct Probe {
        query: isize,
        counter: isize,
    }

    unsafe impl Send for Probe {}

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    impl Probe {
        pub fn new() -> Option<Self> {
            unsafe {
                let mut query = 0isize;
                if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                    return None;
                }
                let mut counter = 0isize;
                let path = wide(r"\GPU Engine(*engtype_3D)\Utilization Percentage");
                if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                PdhCollectQueryData(query);
                Some(Self { query, counter })
            }
        }

        pub fn sample(&mut self) -> Option<f32> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let (mut size, mut count) = (0u32, 0u32);
                PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut());
                if size == 0 {
                    return Some(0.0);
                }
                let mut buf = vec![0u8; size as usize];
                let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, items) != 0 {
                    return None;
                }
                let mut per_gpu: HashMap<String, f64> = HashMap::new();
                for i in 0..count as usize {
                    let it = &*items.add(i);
                    let mut n = 0;
                    while *it.szName.add(n) != 0 {
                        n += 1;
                    }
                    let name = String::from_utf16_lossy(std::slice::from_raw_parts(it.szName, n));
                    // « pid_123_luid_0x…_0x…_phys_0_eng_0_engtype_3D » : la carte = son luid.
                    let luid = name.split("_phys").next().and_then(|s| s.split("luid_").nth(1)).unwrap_or("").to_string();
                    *per_gpu.entry(luid).or_default() += it.FmtValue.Anonymous.doubleValue;
                }
                Some(per_gpu.values().cloned().fold(0.0, f64::max).min(100.0) as f32)
            }
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

#[cfg(not(windows))]
mod gpu {
    // Pas encore de mesure GPU portable sous macOS et Linux.
    pub struct Probe;
    impl Probe {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn sample(&mut self) -> Option<f32> {
            None
        }
    }
}
