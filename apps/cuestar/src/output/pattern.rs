//! Mire de repérage, dessinée sur le CPU (sert aussi quand libmpv est absent).

/// Remplit `buf` (0x00RRGGBB) : barres de couleur, cadre blanc et croix centrale.
pub fn draw(buf: &mut [u32], w: usize, h: usize) {
    const BARS: [u32; 8] = [0xc0c0c0, 0xc0c000, 0x00c0c0, 0x00c000, 0xc000c0, 0xc00000, 0x0000c0, 0x101010];
    for y in 0..h {
        let row = &mut buf[y * w..(y + 1) * w];
        let lower = y > h * 3 / 4;
        for (x, px) in row.iter_mut().enumerate() {
            *px = if lower {
                // Rampe de gris : vérifie la plage (limitée / complète) de la sortie.
                let g = (x * 255 / w.max(1)) as u32;
                (g << 16) | (g << 8) | g
            } else {
                BARS[(x * 8 / w.max(1)).min(7)]
            };
        }
    }
    let border = (w.min(h) / 120).max(2);
    for y in 0..h {
        for x in 0..w {
            let edge = x < border || y < border || x >= w - border || y >= h - border;
            let cross = (x.abs_diff(w / 2) < border / 2 + 1 && y.abs_diff(h / 2) < h / 10)
                || (y.abs_diff(h / 2) < border / 2 + 1 && x.abs_diff(w / 2) < h / 10);
            if edge || cross {
                buf[y * w + x] = 0xffffff;
            }
        }
    }
}
