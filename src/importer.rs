//! Import pets from a downloaded Shimeji / Shimeji-ee zip or folder.
//! Shimeji-ee keeps one shared `conf\` next to `img\`, so copying only `img\<Pet>` loses the
//! behaviour files. The importer copies every pet with its config (and sounds) attached.

use std::fs;
use std::path::{Path, PathBuf};

pub struct Report {
    pub added: Vec<String>,
    pub failed: Vec<String>,
}

fn has_png(dir: &Path) -> bool {
    fs::read_dir(dir).map(|r| r.flatten().any(|e| e.path().extension().map(|x| x.eq_ignore_ascii_case("png")).unwrap_or(false))).unwrap_or(false)
}

fn conf_files(dir: &Path) -> Vec<PathBuf> {
    ["actions.xml", "behaviors.xml", "動作.xml", "行動.xml"].iter().map(|f| dir.join(f)).filter(|p| p.is_file()).collect()
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let p = e.path();
        let d = to.join(e.file_name());
        if p.is_dir() { copy_dir(&p, &d)?; } else { fs::copy(&p, &d)?; }
    }
    Ok(())
}

fn child_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false))
}

/// Folders that look like a Shimeji install (contain `img\`), searched a few levels down.
fn shimeji_roots(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if child_dir(dir, "img").is_some() { out.push(dir.to_path_buf()); return; }
    if depth == 0 { return; }
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() { shimeji_roots(&e.path(), depth - 1, out); }
    }
}

/// "Kopa Cub Shimeji.zip" -> "Kopa Cub"
fn nice_name(source: &Path) -> String {
    let stem = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut n = stem.replace(['_', '-'], " ");
    for w in ["shimeji-ee", "shimeji ee", "shimeji", "shimejis", "mascot"] {
        if let Some(i) = n.to_lowercase().find(w) { n.replace_range(i..i + w.len(), ""); }
    }
    let n = n.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() { "New pet".into() } else { n }
}

fn free_name(img: &Path, base: &str) -> String {
    if !img.join(base).exists() { return base.to_string(); }
    (2..).map(|i| format!("{base} ({i})")).find(|n| !img.join(n).exists()).unwrap()
}

fn extract_zip(zip: &Path, to: &Path) -> Result<(), String> {
    let f = fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut a = zip::ZipArchive::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
    for i in 0..a.len() {
        let mut e = a.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name rejects absolute paths and "..", so nothing escapes the temp folder
        let Some(rel) = e.enclosed_name() else { continue };
        let out = to.join(rel);
        if e.is_dir() { let _ = fs::create_dir_all(&out); continue; }
        if let Some(p) = out.parent() { let _ = fs::create_dir_all(p); }
        let mut w = fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut e, &mut w).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Copy one pet folder into `img\<name>`, attaching `conf` and sounds from the Shimeji install if it has none.
fn add_pet(root_img: &Path, pet: &Path, name: &str, shared_conf: Option<&Path>, sound: Option<&Path>, popeto_root: &Path) -> Result<String, String> {
    let name = free_name(root_img, name);
    let dest = root_img.join(&name);
    copy_dir(pet, &dest).map_err(|e| e.to_string())?;
    if conf_files(&dest.join("conf")).is_empty() {
        let pet_name = pet.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // Shimeji-ee looks in conf\<pet>\ first, then conf\
        let src = shared_conf.and_then(|c| {
            let own = c.join(&pet_name);
            if !conf_files(&own).is_empty() { Some(own) } else if !conf_files(c).is_empty() { Some(c.to_path_buf()) } else { None }
        });
        match src {
            Some(c) => {
                let _ = fs::create_dir_all(dest.join("conf"));
                for f in conf_files(&c) { let _ = fs::copy(&f, dest.join("conf").join(f.file_name().unwrap())); }
            }
            None => {
                let _ = fs::remove_dir_all(&dest);
                return Err(format!("{name}: no actions.xml / behaviors.xml in the package"));
            }
        }
    }
    if let Some(s) = sound { if !dest.join("sound").exists() { let _ = copy_dir(s, &dest.join("sound")); } }
    // make sure Popeto can actually run it
    if let Err(e) = crate::config::load_pack(popeto_root, &dest, 1.0) {
        let _ = fs::remove_dir_all(&dest);
        return Err(format!("{name}: {e}"));
    }
    Ok(name)
}

/// Import from a .zip, or from any file/folder inside an unpacked Shimeji download.
pub fn import(source: &Path, popeto_root: &Path) -> Report {
    let mut rep = Report { added: vec![], failed: vec![] };
    let img = popeto_root.join("img");
    let _ = fs::create_dir_all(&img);
    let is_zip = source.extension().map(|x| x.eq_ignore_ascii_case("zip")).unwrap_or(false);
    let tmp = std::env::temp_dir().join(format!("popeto-import-{}", std::process::id()));
    let base = if is_zip {
        let _ = fs::remove_dir_all(&tmp);
        if let Err(e) = extract_zip(source, &tmp) {
            rep.failed.push(format!("{}: {e}", source.display()));
            return rep;
        }
        tmp.clone()
    } else if source.is_file() {
        // a file inside the download (Shimeji-ee.jar, a png...): use the nearest folder that looks like Shimeji
        let mut d = source.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for _ in 0..3 {
            if child_dir(&d, "img").is_some() || has_png(&d) { break; }
            match d.parent() { Some(p) => d = p.to_path_buf(), None => break }
        }
        d
    } else {
        source.to_path_buf()
    };
    let label = nice_name(if is_zip || source.is_dir() { source } else { &base });

    let mut roots = Vec::new();
    shimeji_roots(&base, 3, &mut roots);
    if roots.is_empty() && has_png(&base) {
        // a single pet folder: images (+ maybe conf) directly inside
        let parent_conf = base.parent().and_then(|p| child_dir(p, "conf"));
        let name = base.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(label.clone());
        match add_pet(&img, &base, &name, parent_conf.as_deref(), None, popeto_root) {
            Ok(n) => rep.added.push(n),
            Err(e) => rep.failed.push(e),
        }
    }
    for root in &roots {
        let conf = child_dir(root, "conf");
        let sound = child_dir(root, "sound");
        let Some(pets_dir) = child_dir(root, "img") else { continue };
        let mut pets: Vec<PathBuf> = fs::read_dir(&pets_dir).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.is_dir() && has_png(p) && !p.file_name().map(|n| n.eq_ignore_ascii_case("unused")).unwrap_or(false))
            .collect();
        pets.sort();
        for pet in pets {
            let own = pet.file_name().unwrap().to_string_lossy().to_string();
            // the stock folder name says nothing; name it after the download instead
            let name = if own.eq_ignore_ascii_case("shimeji") { label.clone() } else { own };
            match add_pet(&img, &pet, &name, conf.as_deref(), sound.as_deref(), popeto_root) {
                Ok(n) => rep.added.push(n),
                Err(e) => rep.failed.push(e),
            }
        }
    }
    if rep.added.is_empty() && rep.failed.is_empty() {
        rep.failed.push(format!("{}: no Shimeji pets found inside", source.display()));
    }
    if is_zip { let _ = fs::remove_dir_all(&tmp); }
    rep
}

#[cfg(test)]
mod tests {
    /// POPETO_IMPORT_ZIP=<zip> POPETO_IMPORT_ROOT=<empty dir> cargo test import_zip -- --nocapture
    #[test]
    fn import_zip() {
        let (Ok(zip), Ok(root)) = (std::env::var("POPETO_IMPORT_ZIP"), std::env::var("POPETO_IMPORT_ROOT")) else { return };
        let rep = super::import(std::path::Path::new(&zip), std::path::Path::new(&root));
        println!("added {:?} failed {:?}", rep.added, rep.failed);
        assert!(!rep.added.is_empty());
    }
}
