//! Mapping 4 coins : homographie appliquée au GPU par un shader utilisateur mpv (hook OUTPUT).

use std::path::PathBuf;

type M3 = [[f64; 3]; 3];

/// Homographie qui envoie le carré unité (u, v) sur le quadrilatère donné
/// (haut-gauche, haut-droit, bas-droit, bas-gauche). Méthode de Heckbert.
fn square_to_quad(q: [[f32; 2]; 4]) -> M3 {
    let [x0, x1, x2, x3] = q.map(|p| p[0] as f64);
    let [y0, y1, y2, y3] = q.map(|p| p[1] as f64);
    let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
    let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);
    let det = dx1 * dy2 - dx2 * dy1;
    let (g, h) = if det.abs() < 1e-12 { (0.0, 0.0) } else { ((dx3 * dy2 - dx2 * dy3) / det, (dx1 * dy3 - dx3 * dy1) / det) };
    [[x1 - x0 + g * x1, x3 - x0 + h * x3, x0], [y1 - y0 + g * y1, y3 - y0 + h * y3, y0], [g, h, 1.0]]
}

/// Position à l'écran du point (u, v) de l'image (aperçu de la régie).
pub fn map_point(corners: [[f32; 2]; 4], u: f32, v: f32) -> [f32; 2] {
    let m = square_to_quad(corners);
    let (u, v) = (u as f64, v as f64);
    let w = m[2][0] * u + m[2][1] * v + m[2][2];
    [((m[0][0] * u + m[0][1] * v + m[0][2]) / w) as f32, ((m[1][0] * u + m[1][1] * v + m[1][2]) / w) as f32]
}

fn invert(m: M3) -> Option<M3> {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let (ca, cb, cc) = (e * i - f * h, -(d * i - f * g), d * h - e * g);
    let det = a * ca + b * cb + c * cc;
    if det.abs() < 1e-12 {
        return None;
    }
    let k = 1.0 / det;
    Some([
        [ca * k, -(b * i - c * h) * k, (b * f - c * e) * k],
        [cb * k, (a * i - c * g) * k, -(a * f - c * d) * k],
        [cc * k, -(a * h - b * g) * k, (a * e - b * d) * k],
    ])
}

/// Source GLSL du shader : pour chaque pixel de l'écran, va chercher le point de l'image d'origine.
/// Avec `fade`, un paramètre réglable à chaud (`glsl-shader-opts=fade=…`, vo gpu-next) assombrit
/// l'image jusqu'au noir : il sert aux transitions.
pub fn shader(corners: [[f32; 2]; 4], fade: bool) -> Option<String> {
    let m = invert(square_to_quad(corners))?;
    let f = |v: f64| format!("{v:.9}");
    let (param, mul) = if fade {
        ("//!PARAM fade\n//!DESC Niveau de l'image (0 = noir)\n//!TYPE float\n//!MINIMUM 0.0\n//!MAXIMUM 1.0\n1.0\n\n", " * vec4(vec3(fade), 1.0)")
    } else {
        ("", "")
    };
    Some(format!(
        "{param}//!HOOK OUTPUT
//!BIND HOOKED
//!DESC CueStar mapping 4 coins
vec4 hook() {{
    vec2 p = HOOKED_pos;
    float w = {g} * p.x + {h} * p.y + {i};
    vec2 uv = vec2({a} * p.x + {b} * p.y + {c}, {d} * p.x + {e} * p.y + {ff}) / w;
    if (w <= 0.0 || uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0)
        return vec4(0.0, 0.0, 0.0, 1.0);
    return HOOKED_tex(uv){mul};
}}
",
        a = f(m[0][0]),
        b = f(m[0][1]),
        c = f(m[0][2]),
        d = f(m[1][0]),
        e = f(m[1][1]),
        ff = f(m[1][2]),
        g = f(m[2][0]),
        h = f(m[2][1]),
        i = f(m[2][2]),
    ))
}

/// Fichiers de shader successifs : mpv met en cache par chemin, chaque réglage prend donc un nom neuf.
pub struct ShaderFiles {
    n: u64,
    last: Option<PathBuf>,
}

impl ShaderFiles {
    pub fn new() -> Self {
        Self { n: 0, last: None }
    }

    pub fn write(&mut self, src: &str) -> std::io::Result<PathBuf> {
        self.n += 1;
        let p = std::env::temp_dir().join(format!("cuestar-warp-{}-{}.glsl", std::process::id(), self.n));
        std::fs::write(&p, src)?;
        self.clear_previous();
        self.last = Some(p.clone());
        Ok(p)
    }

    fn clear_previous(&mut self) {
        if let Some(old) = self.last.take() {
            let _ = std::fs::remove_file(old);
        }
    }
}

impl Drop for ShaderFiles {
    fn drop(&mut self) {
        self.clear_previous();
    }
}
