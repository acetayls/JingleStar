//! Sonde de mesure du moteur de sortie CueStar.
//! probe <cuestar.exe> <scenario.txt> <log.jsonl> [output_left_x] [prefs json]
use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use windows::core::Interface;
use windows::Win32::Foundation::{FILETIME, HANDLE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::GetProcessTimes;

fn qpc() -> i64 {
    let mut v = 0;
    unsafe {
        let _ = QueryPerformanceCounter(&mut v);
    }
    v
}

const PTS: [(u32, u32); 4] = [(100, 100), (960, 540), (1500, 800), (700, 300)];

fn capture(left: i32, tx: mpsc::Sender<String>, stop: Arc<AtomicBool>) -> windows::core::Result<()> {
    unsafe {
        let f: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut found = None;
        let mut i = 0;
        while let Ok(a) = f.EnumAdapters1(i) {
            let mut j = 0;
            while let Ok(o) = a.EnumOutputs(j) {
                let d = o.GetDesc()?;
                if d.DesktopCoordinates.left == left {
                    found = Some((a.clone(), o));
                }
                j += 1;
            }
            i += 1;
        }
        let (adapter, output) = found.expect("output not found");
        let mut dev = None;
        let mut ctx = None;
        D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, Default::default(), D3D11_CREATE_DEVICE_FLAG(0), None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))?;
        let dev: ID3D11Device = dev.unwrap();
        let ctx: ID3D11DeviceContext = ctx.unwrap();
        let o1: IDXGIOutput1 = output.cast()?;
        let mut dup = o1.DuplicateOutput(&dev)?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: PTS.len() as u32,
            Height: 1,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut st = None;
        dev.CreateTexture2D(&desc, None, Some(&mut st))?;
        let st = st.unwrap();
        let mut idle = 0u32;
        while !stop.load(Ordering::Relaxed) {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            match dup.AcquireNextFrame(50, &mut info, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => { idle += 1; if idle % 20 == 0 { let _ = tx.send(json!({"k":"cap_idle","t":qpc(),"n":idle}).to_string()); } continue }
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    let _ = tx.send(json!({"k":"dup_lost","t":qpc()}).to_string());
                    std::thread::sleep(Duration::from_millis(20));
                    if let Ok(d) = o1.DuplicateOutput(&dev) {
                        dup = d;
                    }
                    continue;
                }
                Err(e) => {
                    let _ = tx.send(json!({"k":"dup_err","t":qpc(),"e":e.to_string()}).to_string());
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
            }
            idle = 0;
            let t = qpc();
            if info.LastPresentTime == 0 { let _ = tx.send(json!({"k":"cap_noprez","t":t,"acc":info.AccumulatedFrames}).to_string()); }
            if info.LastPresentTime != 0 {
                if let Some(r) = &res {
                    let tex: ID3D11Texture2D = r.cast()?;
                    for (k, (x, y)) in PTS.iter().enumerate() {
                        let b = D3D11_BOX { left: *x, top: *y, front: 0, right: x + 1, bottom: y + 1, back: 1 };
                        ctx.CopySubresourceRegion(&st, 0, k as u32, 0, 0, &tex, 0, Some(&b));
                    }
                    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                    ctx.Map(&st, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
                    let p = std::slice::from_raw_parts(m.pData as *const u8, PTS.len() * 4);
                    let px: Vec<[u8; 3]> = (0..PTS.len()).map(|k| [p[k * 4 + 2], p[k * 4 + 1], p[k * 4]]).collect();
                    ctx.Unmap(&st, 0);
                    let _ = tx.send(json!({"k":"px","t":t,"pt":info.LastPresentTime,"acc":info.AccumulatedFrames,"px":px}).to_string());
                }
            }
            let _ = dup.ReleaseFrame();
        }
        Ok(())
    }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let exe = &a[1];
    let scen = std::fs::read_to_string(&a[2]).unwrap();
    let logp = &a[3];
    let left: i32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let prefs = a.get(5).cloned().unwrap_or_else(|| "{\"volume\":0.0}".into());
    let mut freq = 0;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut freq);
    }
    let (tx, rx) = mpsc::channel::<String>();
    let mut log = std::io::BufWriter::new(std::fs::File::create(logp).unwrap());
    writeln!(log, "{}", json!({"k":"start","t":qpc(),"freq":freq,"prefs":prefs})).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let tx = tx.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            if let Err(e) = capture(left, tx.clone(), stop) {
                let _ = tx.send(json!({"k":"cap_fail","e":e.to_string()}).to_string());
            }
        });
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut child = Command::new(exe).arg("--output").arg(&prefs).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(match std::env::var_os("PROBE_STDERR") { Some(f) => Stdio::from(std::fs::File::create(f).unwrap()), None => Stdio::null() }).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let (etx, erx) = mpsc::channel::<String>();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                let t = qpc();
                let v: serde_json::Value = serde_json::from_str(&l).unwrap_or(json!(l));
                if let Some(p) = v.get("Position") {
                    let _ = tx.send(json!({"k":"pos","t":t,"time":p["time"]}).to_string());
                    continue;
                }
                let name = match &v { serde_json::Value::Object(m) => m.keys().next().cloned().unwrap_or_default(), serde_json::Value::String(s) => s.clone(), _ => String::new() };
                let _ = etx.send(name);
                let _ = tx.send(json!({"k":"ev","t":t,"ev":v}).to_string());
            }
        });
    }
    {
        let tx = tx.clone();
        let h = child.as_raw_handle() as isize;
        let stop = stop.clone();
        std::thread::spawn(move || {
            let h = HANDLE(h as *mut _);
            while !stop.load(Ordering::Relaxed) {
                let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
                let mut m = PROCESS_MEMORY_COUNTERS_EX::default();
                m.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                unsafe {
                    let _ = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
                    let _ = GetProcessMemoryInfo(h, &mut m as *mut _ as *mut PROCESS_MEMORY_COUNTERS, m.cb);
                }
                let _ = tx.send(json!({"k":"res","t":qpc(),"cpu100ns":ft(k)+ft(u),"ws":m.WorkingSetSize,"priv":m.PrivateUsage}).to_string());
                std::thread::sleep(Duration::from_millis(500));
            }
        });
    }
    let writer = std::thread::spawn(move || {
        for l in rx {
            let _ = writeln!(log, "{l}");
        }
        let _ = log.flush();
    });
    let send = |stdin: &mut std::process::ChildStdin, v: serde_json::Value, tx: &mpsc::Sender<String>| {
        let s = v.to_string();
        let t = qpc();
        let _ = writeln!(stdin, "{s}");
        let _ = stdin.flush();
        let _ = tx.send(json!({"k":"cmd","t":t,"cmd":v}).to_string());
    };
    for line in scen.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.is_empty() || w[0].starts_with('#') {
            continue;
        }
        match w[0] {
            "waitev" => {
                while erx.try_recv().is_ok() {}
                let deadline = std::time::Instant::now() + Duration::from_secs_f64(w[2].parse().unwrap());
                loop {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    match erx.recv_timeout(left) { Ok(n) if n == w[1] => break, Ok(_) => {}, Err(_) => { let _ = tx.send(json!({"k":"timeout","t":qpc(),"what":w[1]}).to_string()); break } }
                }
            }
            "wait" => std::thread::sleep(Duration::from_secs_f64(w[1].parse().unwrap())),
            "mark" => {
                let _ = tx.send(json!({"k":"mark","t":qpc(),"label":w[1..].join(" ")}).to_string());
            }
            "load" => {
                let path = w[1];
                let mut tr = json!({"kind":"Cut","secs":0.0});
                let (mut lp, mut hold, mut paused, mut signal) = (false, false, false, false);
                for o in &w[2..] {
                    if let Some(s) = o.strip_prefix("xfade:") {
                        tr = json!({"kind":"Crossfade","secs":s.parse::<f64>().unwrap()});
                    } else if let Some(s) = o.strip_prefix("black:") {
                        tr = json!({"kind":"Black","secs":s.parse::<f64>().unwrap()});
                    } else if *o == "loop" {
                        lp = true
                    } else if *o == "hold" {
                        hold = true
                    } else if *o == "paused" {
                        paused = true
                    } else if *o == "signal" {
                        signal = true
                    }
                }
                send(&mut stdin, json!({"Load":{"path":path,"paused":paused,"looping":lp,"hold":hold,"transition":tr,"signal_end":signal}}), &tx);
            }
            "prepare" => {
                let lp = w.get(2) == Some(&"loop");
                send(&mut stdin, json!({"Prepare":{"path":w[1],"looping":lp}}), &tx);
            }
            "stop" => send(&mut stdin, json!("Stop"), &tx),
            "play" => send(&mut stdin, json!("Play"), &tx),
            "pause" => send(&mut stdin, json!("Pause"), &tx),
            _ => eprintln!("? {line}"),
        }
    }
    send(&mut stdin, json!("Quit"), &tx);
    std::thread::sleep(Duration::from_millis(1500));
    let _ = child.kill();
    stop.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    drop(tx);
    let _ = writer.join();
}
