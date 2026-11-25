use audiorouter_windows_audio::{resolve_application_group_roots, ApplicationInfo};

#[test]
fn application_group_roots_returns_all_independent_roots() {
    let first = ApplicationInfo {
        process_id: 100,
        parent_process_id: 1,
        executable: "BloodStrike.exe".into(),
        executable_path: Some(r"D:\Games\BloodStrike\BloodStrike.exe".into()),
        creation_time_100ns: Some(10),
    };
    let child = ApplicationInfo {
        process_id: 101,
        parent_process_id: 100,
        executable: "BloodStrike.exe".into(),
        executable_path: Some(r"D:\Games\BloodStrike\BloodStrike.exe".into()),
        creation_time_100ns: Some(11),
    };
    let second = ApplicationInfo {
        process_id: 200,
        parent_process_id: 2,
        executable: "BloodStrike.exe".into(),
        executable_path: Some(r"D:\Games\BloodStrike\BloodStrike.exe".into()),
        creation_time_100ns: Some(12),
    };
    let roots = resolve_application_group_roots(
        &[first, child, second],
        "BloodStrike.exe",
        Some(r"d:\games\bloodstrike\bloodstrike.exe"),
    )
    .unwrap();
    let pids: Vec<u32> = roots.into_iter().map(|app| app.process_id).collect();
    assert_eq!(pids, vec![100, 200]);
}
