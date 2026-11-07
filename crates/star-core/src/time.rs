//! Dates sans dépendance : horodatage du journal et des copies de sauvegarde.

/// Date et heure UTC : « 2026-10-26 08:05:03 UTC », ou « 20261026-080503 » pour un nom de fichier.
pub fn now(for_file: bool) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let (h, mi, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    if for_file {
        format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
    } else {
        format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
    }
}

/// Jours depuis le 1er janvier 1970 → (année, mois, jour), calendrier grégorien (H. Hinnant).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::civil_from_days(0), (1970, 1, 1));
        assert_eq!(super::civil_from_days(20_752), (2026, 10, 26));
        assert_eq!(super::civil_from_days(11_016), (2000, 2, 29));
    }
}
