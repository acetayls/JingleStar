//! Moteur graphique Windows : Direct3D 11, chaîne d'affichage « flip » (sans copie, faible
//! latence), décodage GPU d3d11va sans copie. Voir `screen` pour l'ensemble.

use super::monitor::MonitorFeed;
use super::mpv::{self, Handle, Mpv, RenderApi, RenderParam};
use super::screen::{on_update, Backend, DeckRc, Params, Screen, Shared, THUMB_W};
use std::ffi::{c_int, c_void, CString};
use std::sync::Arc;
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::Threading::WaitForSingleObjectEx;

/// Ouvre l'affichage Direct3D 11 dans la fenêtre `hwnd`, sur la carte reliée à l'écran `hmonitor`.
pub fn open(hwnd: isize, hmonitor: isize, size: (u32, u32), refresh_hz: f64, decks: Vec<Mpv>) -> Result<Screen, String> {
    let (hw, hm) = (super::screen::SendPtr(hwnd), super::screen::SendPtr(hmonitor));
    Screen::start(size, refresh_hz, decks, move |shared, links| {
        let (hw, hm) = (hw, hm);
        unsafe { setup(shared, hw.0, hm.0, links) }
    })
}

const SHADER: &str = r#"
Texture2D tex0 : register(t0);
Texture2D tex1 : register(t1);
SamplerState smp : register(s0);
cbuffer Params : register(b0) {
    float4 h0;      // homographie écran -> image, ligne 1
    float4 h1;      // ligne 2
    float4 h2;      // ligne 3
    float4 misc;    // x = mélange (0 : lecteur 0), y = niveau, z = lecteur 0 visible, w = lecteur 1 visible
};
struct VsOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VsOut vs(uint id : SV_VertexID) {
    VsOut o;
    float2 t = float2((id << 1) & 2, id & 2);
    o.pos = float4(t * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    o.uv = t;
    return o;
}
float4 ps(VsOut i) : SV_Target {
    float2 p = i.uv;
    float w = h2.x * p.x + h2.y * p.y + h2.z;
    float2 uv = float2(h0.x * p.x + h0.y * p.y + h0.z, h1.x * p.x + h1.y * p.y + h1.z) / w;
    if (w <= 0.0 || uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0)
        return float4(0.0, 0.0, 0.0, 1.0);
    float3 a = tex0.Sample(smp, uv).rgb * misc.z;
    float3 b = tex1.Sample(smp, uv).rgb * misc.w;
    return float4(lerp(a, b, misc.x) * misc.y, 1.0);
}
"#;

unsafe fn compile(entry: &str, target: &str) -> Result<ID3DBlob, String> {
    let e = CString::new(entry).unwrap();
    let t = CString::new(target).unwrap();
    let mut code = None;
    let mut errs: Option<ID3DBlob> = None;
    let r = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const c_void,
            SHADER.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(e.as_ptr() as *const u8),
            PCSTR(t.as_ptr() as *const u8),
            0,
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    if let Err(err) = r {
        let msg = errs.map(|b| unsafe { String::from_utf8_lossy(blob(&b)).into_owned() }).unwrap_or_default();
        return Err(format!("shader {entry} : {err} {msg}"));
    }
    code.ok_or_else(|| "shader vide".into())
}

unsafe fn blob(b: &ID3DBlob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
}

fn tex_desc(w: u32, h: u32, usage: D3D11_USAGE, bind: u32, cpu: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: usage,
        BindFlags: bind,
        CPUAccessFlags: cpu,
        MiscFlags: 0,
    }
}

unsafe fn texture(dev: &ID3D11Device, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, String> {
    let mut t = None;
    unsafe { dev.CreateTexture2D(desc, None, Some(&mut t)) }.map_err(|e| format!("texture : {e}"))?;
    t.ok_or_else(|| "texture".into())
}

unsafe fn rtv(dev: &ID3D11Device, t: &ID3D11Texture2D) -> Result<ID3D11RenderTargetView, String> {
    let mut v = None;
    unsafe { dev.CreateRenderTargetView(t, None, Some(&mut v)) }.map_err(|e| format!("vue de rendu : {e}"))?;
    v.ok_or_else(|| "vue de rendu".into())
}

/// Carte graphique reliée à l'écran de sortie (évite une copie entre deux cartes).
unsafe fn adapter_for(hmonitor: isize) -> Option<IDXGIAdapter1> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut i = 0;
    while let Ok(ad) = unsafe { factory.EnumAdapters1(i) } {
        let mut j = 0;
        while let Ok(out) = unsafe { ad.EnumOutputs(j) } {
            if let Ok(d) = unsafe { out.GetDesc() } {
                if d.Monitor == HMONITOR(hmonitor as *mut c_void) {
                    return Some(ad);
                }
            }
            j += 1;
        }
        i += 1;
    }
    None
}

/// Texture d'un lecteur : mpv dessine dedans, le compositeur la lit.
struct Target {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
}

struct D3d {
    gpu: String,
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swap: IDXGISwapChain1,
    waitable: HANDLE,
    size: (u32, u32),
    targets: Vec<Target>,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cb: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    thumb: (ID3D11Texture2D, ID3D11RenderTargetView, u32, u32),
    /// Copies des vignettes en attente de lecture (texture, en attente).
    staging: Vec<(ID3D11Texture2D, bool)>,
    slot: usize,
}

unsafe fn setup(shared: &Arc<Shared>, hwnd: isize, hmonitor: isize, links: &[(Handle, RenderApi)]) -> Result<(D3d, Vec<DeckRc>), String> {
    unsafe {
        let adapter = adapter_for(hmonitor);
        let mut dev: Option<ID3D11Device> = None;
        let mut ctx: Option<ID3D11DeviceContext> = None;
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let r = match &adapter {
            Some(a) => D3D11CreateDevice(a, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), flags, Some(&levels), D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx)),
            None => D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), flags, Some(&levels), D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx)),
        };
        r.map_err(|e| format!("Direct3D 11 : {e}"))?;
        let dev = dev.ok_or("Direct3D 11 : pas de carte")?;
        let ctx = ctx.ok_or("Direct3D 11 : pas de contexte")?;
        let dxgi_adapter = dev.cast::<IDXGIDevice>().and_then(|d| d.GetAdapter()).map_err(|e| e.to_string())?;
        let gpu = dxgi_adapter.GetDesc().map(|d| String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string()).unwrap_or_default();
        let factory: IDXGIFactory2 = dxgi_adapter.GetParent().map_err(|e| e.to_string())?;
        let size = shared.st.lock().map(|s| s.size).unwrap_or((1, 1));
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
        };
        let hw = HWND(hwnd as *mut c_void);
        let swap = factory.CreateSwapChainForHwnd(&dev, hw, &desc, None, None).map_err(|e| format!("chaîne d'affichage : {e}"))?;
        // Alt+Entrée de DXGI désactivé : c'est CueStar qui gère le plein écran.
        let _ = factory.MakeWindowAssociation(hw, DXGI_MWA_NO_ALT_ENTER);
        let swap2: IDXGISwapChain2 = swap.cast().map_err(|e| e.to_string())?;
        swap2.SetMaximumFrameLatency(1).map_err(|e| e.to_string())?;
        let waitable = swap2.GetFrameLatencyWaitableObject();

        // Contextes de rendu mpv sur notre carte.
        let api_type = CString::new("d3d11").unwrap();
        let mut decks = Vec::new();
        for (handle, api) in links {
            let mut dev_ptr: [*mut c_void; 1] = [dev.as_raw()];
            let mut adv: c_int = 1;
            let mut params = [
                RenderParam { kind: mpv::RENDER_PARAM_API_TYPE, data: api_type.as_ptr() as *mut c_void },
                RenderParam { kind: mpv::RENDER_PARAM_D3D11_INIT_PARAMS, data: dev_ptr.as_mut_ptr() as *mut c_void },
                RenderParam { kind: mpv::RENDER_PARAM_ADVANCED_CONTROL, data: &mut adv as *mut c_int as *mut c_void },
                RenderParam::END,
            ];
            let mut rc: Handle = std::ptr::null_mut();
            let r = (api.create)(&mut rc, *handle, params.as_mut_ptr());
            if r < 0 || rc.is_null() {
                for d in &decks {
                    let d: &DeckRc = d;
                    (d.api.free)(d.rc);
                }
                return Err(format!("API de rendu Direct3D 11 de mpv indisponible (code {r})"));
            }
            (api.set_update_callback)(rc, Some(on_update), Arc::as_ptr(shared) as *mut c_void);
            decks.push(DeckRc::new(rc, *handle, *api));
        }

        let vsb = compile("vs", "vs_4_0")?;
        let psb = compile("ps", "ps_4_0")?;
        let mut vs = None;
        dev.CreateVertexShader(blob(&vsb), None, Some(&mut vs)).map_err(|e| e.to_string())?;
        let mut ps = None;
        dev.CreatePixelShader(blob(&psb), None, Some(&mut ps)).map_err(|e| e.to_string())?;
        let cbd = D3D11_BUFFER_DESC { ByteWidth: std::mem::size_of::<Params>() as u32, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, ..Default::default() };
        let mut cb = None;
        dev.CreateBuffer(&cbd, None, Some(&mut cb)).map_err(|e| e.to_string())?;
        let sd = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        let mut sampler = None;
        dev.CreateSamplerState(&sd, Some(&mut sampler)).map_err(|e| e.to_string())?;
        let thumb = make_thumb(&dev, size)?;
        let mut g = D3d {
            gpu,
            dev,
            ctx,
            swap,
            waitable,
            size,
            targets: Vec::new(),
            vs: vs.ok_or("vs")?,
            ps: ps.ok_or("ps")?,
            cb: cb.ok_or("cb")?,
            sampler: sampler.ok_or("sampler")?,
            thumb,
            staging: Vec::new(),
            slot: 0,
        };
        g.make_targets(decks.len())?;
        Ok((g, decks))
    }
}

unsafe fn make_thumb(dev: &ID3D11Device, size: (u32, u32)) -> Result<(ID3D11Texture2D, ID3D11RenderTargetView, u32, u32), String> {
    let tw = THUMB_W.min(size.0).max(2);
    let th = ((size.1 as u64 * tw as u64 / size.0.max(1) as u64) as u32).max(2) & !1;
    let t = unsafe { texture(dev, &tex_desc(tw, th, D3D11_USAGE_DEFAULT, D3D11_BIND_RENDER_TARGET.0 as u32, 0)) }?;
    let v = unsafe { rtv(dev, &t) }?;
    Ok((t, v, tw, th))
}

impl D3d {
    unsafe fn make_targets(&mut self, n: usize) -> Result<(), String> {
        unsafe {
            self.targets.clear();
            for _ in 0..n {
                let tex = texture(&self.dev, &tex_desc(self.size.0, self.size.1, D3D11_USAGE_DEFAULT, (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32, 0))?;
                // Noir au départ.
                self.ctx.ClearRenderTargetView(&rtv(&self.dev, &tex)?, &[0.0, 0.0, 0.0, 1.0]);
                let mut srv = None;
                self.dev.CreateShaderResourceView(&tex, None, Some(&mut srv)).map_err(|e| format!("vue de texture : {e}"))?;
                self.targets.push(Target { tex, srv: srv.ok_or("vue de texture")? });
            }
            let (tw, th) = (self.thumb.2, self.thumb.3);
            self.staging.clear();
            for _ in 0..3 {
                self.staging.push((texture(&self.dev, &tex_desc(tw, th, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ.0 as u32))?, false));
            }
            self.slot = 0;
            Ok(())
        }
    }

    unsafe fn draw(&self, target: &ID3D11RenderTargetView, w: u32, h: u32, p: &Params) {
        unsafe {
            let c = &self.ctx;
            // mpv ne rétablit pas l'état de Direct3D : on repose tout.
            c.ClearState();
            c.UpdateSubresource(&self.cb, 0, None, p as *const Params as *const c_void, 0, 0);
            c.OMSetRenderTargets(Some(&[Some(target.clone())]), None);
            c.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
            c.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            c.VSSetShader(&self.vs, None);
            c.PSSetShader(&self.ps, None);
            c.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            c.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            let srvs: Vec<Option<ID3D11ShaderResourceView>> = self.targets.iter().map(|t| Some(t.srv.clone())).collect();
            c.PSSetShaderResources(0, Some(&srvs));
            c.Draw(3, 0);
            c.PSSetShaderResources(0, Some(&[None, None]));
            c.OMSetRenderTargets(None, None);
        }
    }
}

impl Backend for D3d {
    fn gpu(&self) -> String {
        self.gpu.clone()
    }

    fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        unsafe {
            self.ctx.ClearState();
            self.ctx.Flush();
            self.swap
                .ResizeBuffers(0, size.0, size.1, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0))
                .map_err(|e| format!("redimensionnement : {e}"))?;
            self.size = size;
            self.thumb = make_thumb(&self.dev, size)?;
            let n = self.targets.len();
            self.make_targets(n)
        }
    }

    fn wait_ready(&mut self) {
        unsafe { WaitForSingleObjectEx(self.waitable, 100, true) };
    }

    fn draw_deck(&mut self, i: usize, d: &DeckRc) {
        unsafe {
            // La texture ne doit plus être liée en lecture pendant que mpv dessine dedans.
            self.ctx.PSSetShaderResources(0, Some(&[None, None]));
            let mut fbo: [*mut c_void; 1] = [self.targets[i].tex.as_raw()];
            let mut block: c_int = 0;
            let mut rp = [
                RenderParam { kind: mpv::RENDER_PARAM_D3D11_FBO, data: fbo.as_mut_ptr() as *mut c_void },
                RenderParam { kind: mpv::RENDER_PARAM_BLOCK_FOR_TARGET_TIME, data: &mut block as *mut c_int as *mut c_void },
                RenderParam::END,
            ];
            (d.api.render)(d.rc, rp.as_mut_ptr());
        }
    }

    fn compose(&mut self, p: &Params) {
        unsafe {
            let Ok(back) = self.swap.GetBuffer::<ID3D11Texture2D>(0) else { return };
            if let Ok(v) = rtv(&self.dev, &back) {
                self.draw(&v, self.size.0, self.size.1, p);
            }
        }
    }

    fn compose_thumb(&mut self, p: &Params) {
        unsafe {
            let (t, v, w, h) = (self.thumb.0.clone(), self.thumb.1.clone(), self.thumb.2, self.thumb.3);
            self.draw(&v, w, h, p);
            let slot = self.slot;
            // Si la copie précédente de cet emplacement n'est pas encore lue, on saute cette vignette.
            if !self.staging[slot].1 {
                self.ctx.CopyResource(&self.staging[slot].0, &t);
                self.staging[slot].1 = true;
                self.slot = (slot + 1) % self.staging.len();
            }
        }
    }

    fn present(&mut self) {
        let _ = unsafe { self.swap.Present(1, DXGI_PRESENT(0)) };
    }

    fn read_thumbs(&mut self, feed: &mut MonitorFeed) {
        let (w, h) = (self.thumb.2 as usize, self.thumb.3 as usize);
        let n = self.staging.len();
        for i in 0..n {
            // La plus ancienne d'abord.
            let k = (self.slot + i) % n;
            if !self.staging[k].1 {
                continue;
            }
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            if unsafe { self.ctx.Map(&self.staging[k].0, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut m)) }.is_err() {
                continue;
            }
            let pitch = m.RowPitch as usize;
            let src = unsafe { std::slice::from_raw_parts(m.pData as *const u8, pitch * h) };
            let mut rgba = vec![0u8; w * h * 4];
            for y in 0..h {
                let s = &src[y * pitch..y * pitch + w * 4];
                let d = &mut rgba[y * w * 4..(y + 1) * w * 4];
                for (o, i) in d.chunks_exact_mut(4).zip(s.chunks_exact(4)) {
                    o[0] = i[2];
                    o[1] = i[1];
                    o[2] = i[0];
                    o[3] = 255;
                }
            }
            unsafe { self.ctx.Unmap(&self.staging[k].0, 0) };
            self.staging[k].1 = false;
            feed.send(w, h, w * 4, &rgba);
        }
    }
}

impl Drop for D3d {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.waitable) };
    }
}
