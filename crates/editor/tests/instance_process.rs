use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use keine_editor::instance::{OpenRequest, Startup, acquire_or_forward};

const CHILD_ENV: &str = "KEINE_EDITOR_INSTANCE_CHILD";
const ROOT_ENV: &str = "KEINE_EDITOR_INSTANCE_ROOT";
const PROJECT_ENV: &str = "KEINE_EDITOR_INSTANCE_PROJECT";

#[test]
fn secondary_launch_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let root = PathBuf::from(std::env::var_os(ROOT_ENV).unwrap());
    let project = PathBuf::from(std::env::var_os(PROJECT_ENV).unwrap());
    assert!(matches!(
        acquire_or_forward(&root, vec![project]).unwrap(),
        Startup::Forwarded
    ));
}

#[test]
fn secondary_process_forwards_a_real_open_request() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("keine-editor-instance-{nonce}"));
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let Startup::Primary(primary) = acquire_or_forward(&root, Vec::new()).unwrap() else {
        panic!("test process must own the primary instance lock")
    };

    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("secondary_launch_child")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env(ROOT_ENV, &root)
        .env(PROJECT_ENV, &project)
        .spawn()
        .unwrap();

    assert_eq!(
        primary.receiver().receive().unwrap(),
        OpenRequest {
            paths: vec![project]
        }
    );
    assert!(child.wait().unwrap().success());
    drop(primary);
    fs::remove_dir_all(root).unwrap();
}
