// Intègre l'icône dans l'exécutable Windows et la version de build (hash git, date).
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/jinglestar.ico");
    build_info();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/jinglestar.ico");
        res.set("ProductName", "JingleStar");
        res.set("FileDescription", "JingleStar");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône Windows non intégrée : {e}");
        }
    }
}

/// `JINGLESTAR_GIT` (hash court, « + » si modifié) et `JINGLESTAR_BUILD_DATE` (AAAA-MM-JJ, UTC).
fn build_info() {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let mut hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "?".into());
    if git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty()) {
        hash.push('+');
    }
    // On relance quand le commit courant change.
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed=.git/{r}");
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    println!("cargo:rustc-env=JINGLESTAR_GIT={hash}");
    println!("cargo:rustc-env=JINGLESTAR_BUILD_DATE={y:04}-{m:02}-{d:02}");
}

/// Jours depuis 1970-01-01 vers (année, mois, jour), algorithme de H. Hinnant.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
