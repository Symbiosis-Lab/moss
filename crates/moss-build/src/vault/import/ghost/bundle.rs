//! Bounded reads from an export directory or zip; never unpack an archive.
use std::{fs, io::Read, path::{Component, Path, PathBuf}};
use serde_json::Value;

const MAX_FILE: u64 = 128 * 1024 * 1024;
pub(crate) struct Bundle {
    root: PathBuf,
    archive: Option<zip::ZipArchive<fs::File>>,
    pub names: Vec<String>,
    prefix: String,
    pub data: Value,
    site: Option<Value>,
}

impl Bundle {
    pub fn open(path: &Path) -> Result<Option<Self>, String> {
        let root = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let archive = if root.is_dir() { None } else {
            let mut file = fs::File::open(&root).map_err(|e| e.to_string())?; // allow:raw_read an export the person handed to the import, read once outside any site folder
            let mut signature = [0; 4];
            if file.read_exact(&mut signature).is_err() || !matches!(&signature, b"PK\x03\x04" | b"PK\x05\x06") { return Ok(None); }
            Some(zip::ZipArchive::new(fs::File::open(&root).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?) // allow:raw_read an export the person handed to the import, read once outside any site folder
        };
        let names = if let Some(zip) = &archive {
            zip.file_names().map(|n| n.map(|s| s.into_owned())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
        } else {
            walkdir::WalkDir::new(&root).max_depth(12).into_iter().filter_map(Result::ok)
                .filter(|e| e.file_type().is_file()).map(|e| e.path().strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect()
        };
        if names.len() > 100_000 { return Err("Export has too many entries".into()); }
        for name in &names { safe_path(name)?; }
        let mut bundle = Self { root, archive, names, prefix: String::new(), data: Value::Null, site: None };
        let candidates: Vec<_> = bundle.names.iter().filter(|n| n.ends_with(".json") && !n.contains("content/images/") && !n.ends_with("members.json") && !n.ends_with("comments.json")).cloned().collect();
        let mut found = false;
        for name in candidates {
            let Some(bytes) = bundle.read_exact(&name)? else { continue };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if value.pointer("/db/0/data/posts").is_some_and(Value::is_array) {
                if found { return Err("Export contains multiple Ghost databases".into()); }
                bundle.data = value.pointer("/db/0/data").unwrap().clone();
                bundle.prefix = name.rsplit_once('/').map_or(String::new(), |(p, _)| format!("{p}/"));
                found = true;
            }
        }
        if !found { return Ok(None); }
        bundle.site = bundle.read("site.json")?.and_then(|b| serde_json::from_slice(&b).ok());
        Ok(Some(bundle))
    }

    pub fn root(&self) -> &Path { &self.root }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.data["settings"].as_array()?.iter().find(|v| v["key"].as_str() == Some(key))?["value"].as_str().map(str::to_string)
    }
    pub fn site_url(&self) -> Option<String> {
        self.site.as_ref().and_then(|v| v["url"].as_str()).map(str::to_string).or_else(|| self.setting("url"))
    }
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        safe_path(name)?;
        self.read_exact(&format!("{}{name}", self.prefix))
    }
    fn read_exact(&mut self, name: &str) -> Result<Option<Vec<u8>>, String> {
        if !self.names.iter().any(|n| n == name) { return Ok(None); }
        let mut bytes = Vec::new();
        if let Some(zip) = &mut self.archive {
            let file = zip.by_name(name).map_err(|e| e.to_string())?;
            if file.is_symlink() { return Err("Export contains a symbolic link".into()); }
            if file.size() > MAX_FILE { return Err("Export entry exceeds size limit".into()); }
            file.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        } else {
            let path = fs::canonicalize(self.root.join(name)).map_err(|e| e.to_string())?;
            if !path.starts_with(&self.root) { return Err("Export entry leaves its directory".into()); }
            fs::File::open(path).map_err(|e| e.to_string())?.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?; // allow:raw_read an export the person handed to the import, read once outside any site folder
        }
        if bytes.len() as u64 > MAX_FILE { return Err("Export entry exceeds size limit".into()); }
        Ok(Some(bytes))
    }
}

fn safe_path(name: &str) -> Result<(), String> {
    if name.contains('\\') || name.contains(':') || Path::new(name).components().any(|p| !matches!(p, Component::Normal(_))) {
        return Err("Unsafe path in export".into());
    }
    Ok(())
}
