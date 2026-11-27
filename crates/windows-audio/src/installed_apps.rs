//! Discovers installed Windows desktop applications and games (Steam, Epic
//! Games, Start Menu shortcuts, and Uninstall registry entries) so users can
//! search and group applications even when they are not currently running.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledApplicationInfo {
    pub display_name: String,
    pub executable: String,
    pub executable_path: String,
    pub source: &'static str,
}

const INSTALLED_CACHE_TTL: Duration = Duration::from_secs(60);

type InstalledAppsCache = Mutex<Option<(Instant, Vec<InstalledApplicationInfo>)>>;

#[cfg(windows)]
pub fn enumerate_installed_applications() -> Vec<InstalledApplicationInfo> {
    static CACHE: OnceLock<InstalledAppsCache> = OnceLock::new();
    static REFRESHING: AtomicBool = AtomicBool::new(false);
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut needs_refresh = false;
    let snapshot = if let Ok(guard) = cache.lock() {
        match &*guard {
            Some((captured_at, list)) => {
                if captured_at.elapsed() >= INSTALLED_CACHE_TTL {
                    needs_refresh = true;
                }
                list.clone()
            }
            None => {
                needs_refresh = true;
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    if needs_refresh
        && REFRESHING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        let _ = std::thread::Builder::new()
            .name("audiorouter-installed-apps".into())
            .spawn(move || {
                let list = collect_installed_applications();
                if let Ok(mut guard) = cache.lock() {
                    *guard = Some((Instant::now(), list));
                }
                REFRESHING.store(false, Ordering::Release);
            });
    }

    snapshot
}

#[cfg(not(windows))]
pub fn enumerate_installed_applications() -> Vec<InstalledApplicationInfo> {
    Vec::new()
}

#[cfg(windows)]
fn collect_installed_applications() -> Vec<InstalledApplicationInfo> {
    let mut by_path = BTreeMap::<String, InstalledApplicationInfo>::new();
    let mut insert = |entry: InstalledApplicationInfo| {
        let key = entry.executable_path.to_ascii_lowercase();
        by_path.entry(key).or_insert(entry);
    };

    for entry in discover_steam_games() {
        insert(entry);
    }
    for entry in discover_epic_games() {
        insert(entry);
    }
    for entry in discover_start_menu_shortcuts() {
        insert(entry);
    }
    for entry in discover_uninstall_registry() {
        insert(entry);
    }

    let mut list: Vec<_> = by_path.into_values().collect();
    list.sort_by(|left, right| {
        left.display_name
            .to_ascii_lowercase()
            .cmp(&right.display_name.to_ascii_lowercase())
            .then_with(|| left.executable_path.cmp(&right.executable_path))
    });
    list
}

fn is_ignored_executable_name(exe_name: &str) -> bool {
    let lower = exe_name.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return true;
    }
    let stem = lower.trim_end_matches(".exe");
    const EXACT_IGNORED: &[&str] = &[
        "unins000",
        "uninst",
        "uninstall",
        "setup",
        "install",
        "installer",
        "cleanup",
        "touchup",
        "dxsetup",
        "vcredist_x64",
        "vcredist_x86",
        "vc_redist.x64",
        "vc_redist.x86",
        "crashhandler",
        "crashreporter",
        "crashpad_handler",
        "ueprereqsetup_x64",
        "eaanticheat.installer",
        "easyanticheat_setup",
        "easyanticheat_eos_setup",
        "microsoftedgewebview2setup",
        "cmd",
        "powershell",
        "pwsh",
        "conhost",
        "rundll32",
        "regsvr32",
        "msiexec",
        "explorer",
    ];
    if EXACT_IGNORED.contains(&stem) {
        return true;
    }
    stem.starts_with("unins")
        || stem.contains("uninstall")
        || stem.contains("vcredist")
        || stem.contains("crashreport")
        || stem.contains("crashhandler")
        || stem.ends_with("installer")
        || stem.ends_with("_setup")
}

fn clean_exe_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    // Strip trailing ",0" icon index or arguments after ".exe"
    let lower = trimmed.to_ascii_lowercase();
    let exe_end = lower.find(".exe")?;
    let path_str = trimmed[..exe_end + 4].trim_matches('"').trim();
    if path_str.is_empty() {
        return None;
    }
    let path = PathBuf::from(path_str);
    let file_name = path.file_name()?.to_str()?;
    if is_ignored_executable_name(file_name) || !path.is_file() {
        return None;
    }
    Some(path)
}

#[cfg(windows)]
fn read_reg_string(
    root: windows::Win32::System::Registry::HKEY,
    subkey: &str,
    value_name: &str,
) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_EXPAND_SZ, REG_SZ,
        REG_VALUE_TYPE,
    };

    let subkey_w: Vec<u16> = OsStr::new(subkey).encode_wide().chain(Some(0)).collect();
    let value_w: Vec<u16> = OsStr::new(value_name)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut hkey = HKEY::default();
    // SAFETY: `subkey_w` is null-terminated; `hkey` is closed below if opened.
    let open_res = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if open_res.is_err() {
        return None;
    }
    let mut reg_type = REG_VALUE_TYPE::default();
    let mut bytes_len = 0u32;
    // SAFETY: queries required buffer size with null data pointer.
    let size_res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_w.as_ptr()),
            None,
            Some(&mut reg_type),
            None,
            Some(&mut bytes_len),
        )
    };
    if size_res.is_err() || bytes_len == 0 || (reg_type != REG_SZ && reg_type != REG_EXPAND_SZ) {
        // SAFETY: `hkey` was opened by `RegOpenKeyExW` above and is closed once.
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        return None;
    }
    let mut buf = vec![0u16; (bytes_len as usize / 2) + 1];
    let mut actual_bytes = (buf.len() * 2) as u32;
    // SAFETY: `buf` is sized for `actual_bytes` and `hkey` is open.
    let read_res = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value_w.as_ptr()),
            None,
            Some(&mut reg_type),
            Some(buf.as_mut_ptr().cast::<u8>()),
            Some(&mut actual_bytes),
        )
    };
    // SAFETY: `hkey` was opened by `RegOpenKeyExW` above and is closed once.
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if read_res.is_err() {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf16(&buf[..end]).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(windows)]
fn discover_steam_games() -> Vec<InstalledApplicationInfo> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut steam_roots = Vec::new();
    for (root, key, val) in [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ] {
        if let Some(path) = read_reg_string(root, key, val) {
            let pb = PathBuf::from(path.replace('/', "\\"));
            if pb.is_dir() && !steam_roots.iter().any(|p: &PathBuf| p == &pb) {
                steam_roots.push(pb);
            }
        }
    }

    let mut libraries = Vec::new();
    for root in &steam_roots {
        if !libraries.iter().any(|p: &PathBuf| p == root) {
            libraries.push(root.clone());
        }
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(content) = std::fs::read_to_string(&vdf) {
            for line in content.lines() {
                if let Some(path) = parse_vdf_kv(line, "path") {
                    let unescaped = path.replace("\\\\", "\\");
                    let pb = PathBuf::from(unescaped);
                    if pb.is_dir() && !libraries.iter().any(|p| p == &pb) {
                        libraries.push(pb);
                    }
                }
            }
        }
    }

    let mut results = Vec::new();
    for library in libraries {
        let steamapps = library.join("steamapps");
        let Ok(entries) = std::fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut app_name = None;
            let mut install_dir = None;
            for line in content.lines() {
                if app_name.is_none() {
                    app_name = parse_vdf_kv(line, "name");
                }
                if install_dir.is_none() {
                    install_dir = parse_vdf_kv(line, "installdir");
                }
            }
            let (Some(app_name), Some(install_dir)) = (app_name, install_dir) else {
                continue;
            };
            if app_name
                .to_ascii_lowercase()
                .contains("steamworks common redistributables")
            {
                continue;
            }
            let game_dir = steamapps.join("common").join(&install_dir);
            if !game_dir.is_dir() {
                continue;
            }
            let exes = collect_game_exes(&game_dir, 4);
            for exe in exes {
                let Some(file_name) = exe.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let label = if file_name.eq_ignore_ascii_case("launcher.exe") {
                    format!("{app_name} (Launcher)")
                } else {
                    app_name.clone()
                };
                results.push(InstalledApplicationInfo {
                    display_name: label,
                    executable: file_name.to_owned(),
                    executable_path: exe.to_string_lossy().into_owned(),
                    source: "steam",
                });
            }
        }
    }
    results
}

fn parse_vdf_kv(line: &str, key: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && in_quotes {
            if let Some(next) = chars.next() {
                current.push('\\');
                current.push(next);
            }
            continue;
        }
        if ch == '"' {
            if in_quotes {
                parts.push(std::mem::take(&mut current));
                in_quotes = false;
            } else {
                in_quotes = true;
            }
            continue;
        }
        if in_quotes {
            current.push(ch);
        }
    }
    if parts.len() >= 2 && parts[0].eq_ignore_ascii_case(key) {
        Some(parts[1].clone())
    } else {
        None
    }
}

fn collect_game_exes(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_game_exes_inner(dir, 0, max_depth, &mut out);
    // Keep at most 4 candidate exes per game folder to avoid flooding with tools.
    out.sort();
    out.truncate(4);
    out
}

fn collect_game_exes_inner(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth || out.len() >= 16 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if dir_name.starts_with("__")
                || dir_name == "redist"
                || dir_name == "commonredist"
                || dir_name == "thirdparty"
                || dir_name == "directx"
            {
                continue;
            }
            collect_game_exes_inner(&path, depth + 1, max_depth, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if !is_ignored_executable_name(file_name) {
                    out.push(path);
                }
            }
        }
    }
}

#[cfg(windows)]
fn discover_epic_games() -> Vec<InstalledApplicationInfo> {
    let manifests_dir = PathBuf::from(r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests");
    let Ok(entries) = std::fs::read_dir(&manifests_dir) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_none_or(|e| !e.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(display_name) = json.get("DisplayName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(install_location) = json.get("InstallLocation").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(launch_exe) = json.get("LaunchExecutable").and_then(|v| v.as_str()) else {
            continue;
        };
        let full_path = Path::new(install_location).join(launch_exe);
        if let Some(cleaned) = clean_exe_path(&full_path.to_string_lossy()) {
            if let Some(file_name) = cleaned.file_name().and_then(|n| n.to_str()) {
                results.push(InstalledApplicationInfo {
                    display_name: display_name.to_owned(),
                    executable: file_name.to_owned(),
                    executable_path: cleaned.to_string_lossy().into_owned(),
                    source: "epic",
                });
            }
        }
    }
    results
}

#[cfg(windows)]
fn discover_start_menu_shortcuts() -> Vec<InstalledApplicationInfo> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows_core::{Interface, PCWSTR};

    let mut roots = Vec::new();
    if let Ok(program_data) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(program_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(app_data) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(app_data).join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    let mut lnk_files = Vec::new();
    for root in roots {
        collect_lnk_files(&root, 0, 4, &mut lnk_files);
    }
    if lnk_files.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    // SAFETY: COM is initialized for this thread and uninitialized before returning.
    unsafe {
        let coinit = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(shell_link): Result<IShellLinkW, _> =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            if coinit.is_ok() {
                CoUninitialize();
            }
            return results;
        };
        let Ok(persist): Result<IPersistFile, _> = shell_link.cast() else {
            if coinit.is_ok() {
                CoUninitialize();
            }
            return results;
        };
        for lnk in lnk_files {
            use std::ffi::OsStr;
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = OsStr::new(&lnk).encode_wide().chain(Some(0)).collect();
            if persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).is_err() {
                continue;
            }
            let mut target = [0u16; 1024];
            if shell_link
                .GetPath(&mut target, std::ptr::null_mut(), 0)
                .is_err()
            {
                continue;
            }
            let len = target.iter().position(|&c| c == 0).unwrap_or(target.len());
            let Ok(target_str) = String::from_utf16(&target[..len]) else {
                continue;
            };
            let Some(cleaned) = clean_exe_path(&target_str) else {
                continue;
            };
            let Some(exe_name) = cleaned.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let display_name = lnk
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(exe_name)
                .to_owned();
            if display_name.to_ascii_lowercase().contains("uninstall") {
                continue;
            }
            results.push(InstalledApplicationInfo {
                display_name,
                executable: exe_name.to_owned(),
                executable_path: cleaned.to_string_lossy().into_owned(),
                source: "installed",
            });
        }
        if coinit.is_ok() {
            CoUninitialize();
        }
    }
    results
}

fn collect_lnk_files(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth || out.len() >= 512 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_files(&path, depth + 1, max_depth, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            out.push(path);
        }
    }
}

#[cfg(windows)]
fn discover_uninstall_registry() -> Vec<InstalledApplicationInfo> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };

    let roots = [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ];

    let mut results = Vec::new();
    for (root, subkey) in roots {
        let subkey_w: Vec<u16> = OsStr::new(subkey).encode_wide().chain(Some(0)).collect();
        let mut hkey = HKEY::default();
        // SAFETY: `subkey_w` is null-terminated; `hkey` is closed below.
        let open_res = unsafe {
            RegOpenKeyExW(
                root,
                PCWSTR(subkey_w.as_ptr()),
                Some(0),
                KEY_READ,
                &mut hkey,
            )
        };
        if open_res.is_err() {
            continue;
        }
        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 512];
            let mut name_len = name_buf.len() as u32;
            // SAFETY: `name_buf` is sized for `name_len` UTF-16 code units.
            let enum_res = unsafe {
                RegEnumKeyExW(
                    hkey,
                    index,
                    Some(PWSTR(name_buf.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if enum_res.is_err() {
                break;
            }
            index += 1;
            let Ok(child_name) = String::from_utf16(&name_buf[..name_len as usize]) else {
                continue;
            };
            let full_subkey = format!(r"{subkey}\{child_name}");
            let Some(display_name) = read_reg_string(root, &full_subkey, "DisplayName") else {
                continue;
            };
            let exe_path = read_reg_string(root, &full_subkey, "DisplayIcon")
                .and_then(|icon| clean_exe_path(&icon));
            let Some(cleaned) = exe_path else {
                continue;
            };
            let Some(exe_name) = cleaned.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            results.push(InstalledApplicationInfo {
                display_name,
                executable: exe_name.to_owned(),
                executable_path: cleaned.to_string_lossy().into_owned(),
                source: "installed",
            });
        }
        // SAFETY: `hkey` was opened by `RegOpenKeyExW` above and is closed once.
        unsafe {
            let _ = RegCloseKey(hkey);
        }
    }
    results
}
