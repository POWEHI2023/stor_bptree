use std::env;
use std::fs;
use std::path::Path;

use serial_test::serial;
use stor_bptree::bptree::PAGE_SIZE;
use stor_bptree::bptree::{PageRuntimeError, RawPage};
use tempfile::TempDir;

fn set_metafile(path: &Path) {
    unsafe {
        env::set_var("METAFILE", path);
    }
}

fn clear_metafile() {
    unsafe {
        env::remove_var("METAFILE");
    }
}

fn write_page_file(dir: &Path, file_name: &str, len: usize) {
    fs::write(dir.join(file_name), vec![0; len]).unwrap();
}

fn write_main_config(
    dir: &Path,
    global_meta_file: &Path,
    page_file_dir: &Path,
) -> std::path::PathBuf {
    let config_path = dir.join("config.yaml");
    fs::write(
        &config_path,
        format!(
            "GLOBAL_META_FILE: {}\nPAGE_FILE_DIR: {}\n",
            global_meta_file.display(),
            page_file_dir.display()
        ),
    )
    .unwrap();
    config_path
}

fn setup_with_global_meta(global_meta: &str) -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let page_dir = temp.path().join("pages");
    fs::create_dir(&page_dir).unwrap();
    let global_meta_file = temp.path().join("page_map.yaml");
    fs::write(&global_meta_file, global_meta).unwrap();
    let config_path = write_main_config(temp.path(), &global_meta_file, &page_dir);
    (temp, config_path, page_dir)
}

#[test]
#[serial]
fn load_meta_returns_page_info_map() {
    clear_metafile();
    let global_meta = format!(
        "1:\n  file_name: pages.dat\n  offset: 0\n  page_size: {PAGE_SIZE}\n2:\n  file_name: pages.dat\n  offset: {PAGE_SIZE}\n  page_size: {PAGE_SIZE}\n"
    );
    let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE * 2);
    set_metafile(&config_path);

    let (page_meta, _) = RawPage::load_meta().unwrap();

    assert_eq!(page_meta.len(), 2);
    assert_eq!(page_meta[&1].file_name, "pages.dat");
    assert_eq!(page_meta[&1].offset, 0);
    assert_eq!(page_meta[&1].page_size, PAGE_SIZE);
    assert_eq!(page_meta[&2].offset, PAGE_SIZE as u64);
}

#[test]
#[serial]
fn load_meta_requires_metafile_env() {
    clear_metafile();
    let temp = TempDir::new().unwrap();
    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp.path()).unwrap();

    let result = RawPage::load_meta();

    env::set_current_dir(original_dir).unwrap();
    assert!(matches!(
        result,
        Err(PageRuntimeError::MissingEnv { key: "METAFILE" })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_main_config_missing_fields() {
    clear_metafile();
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.yaml");
    fs::write(&config_path, "GLOBAL_META_FILE: page_map.yaml\n").unwrap();
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::Yaml { .. })
    ));

    fs::write(&config_path, "PAGE_FILE_DIR: pages\n").unwrap();

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::Yaml { .. })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_missing_page_file_dir() {
    clear_metafile();
    let temp = TempDir::new().unwrap();
    let global_meta_file = temp.path().join("page_map.yaml");
    let page_dir = temp.path().join("missing-pages");
    fs::write(&global_meta_file, "{}\n").unwrap();
    let config_path = write_main_config(temp.path(), &global_meta_file, &page_dir);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::Io { .. })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_missing_or_invalid_global_meta_file() {
    clear_metafile();
    let temp = TempDir::new().unwrap();
    let page_dir = temp.path().join("pages");
    fs::create_dir(&page_dir).unwrap();
    let missing_global_meta = temp.path().join("missing.yaml");
    let config_path = write_main_config(temp.path(), &missing_global_meta, &page_dir);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::Io { .. })
    ));

    let invalid_global_meta = temp.path().join("invalid.yaml");
    fs::write(&invalid_global_meta, ":\n").unwrap();
    let config_path = write_main_config(temp.path(), &invalid_global_meta, &page_dir);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::Yaml { .. })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_zero_page_id() {
    clear_metafile();
    let global_meta =
        format!("0:\n  file_name: pages.dat\n  offset: 0\n  page_size: {PAGE_SIZE}\n");
    let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::InvalidMeta {
            page_id: Some(0),
            ..
        })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_invalid_file_names() {
    for file_name in ["/tmp/pages.dat", "nested/pages.dat", "../pages.dat"] {
        clear_metafile();
        let global_meta =
            format!("1:\n  file_name: {file_name:?}\n  offset: 0\n  page_size: {PAGE_SIZE}\n");
        let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
        write_page_file(&page_dir, "pages.dat", PAGE_SIZE);
        set_metafile(&config_path);

        assert!(matches!(
            RawPage::load_meta(),
            Err(PageRuntimeError::InvalidPath { .. })
        ));
    }
}

#[test]
#[serial]
fn load_meta_rejects_wrong_page_size() {
    clear_metafile();
    let global_meta = "1:\n  file_name: pages.dat\n  offset: 0\n  page_size: 4096\n";
    let (_temp, config_path, page_dir) = setup_with_global_meta(global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::InvalidMeta {
            page_id: Some(1),
            ..
        })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_file_range_out_of_bounds() {
    clear_metafile();
    let global_meta =
        format!("1:\n  file_name: pages.dat\n  offset: 1\n  page_size: {PAGE_SIZE}\n");
    let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::InvalidMeta {
            page_id: Some(1),
            ..
        })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_overflowing_file_range() {
    clear_metafile();
    let global_meta = format!(
        "1:\n  file_name: pages.dat\n  offset: {}\n  page_size: {PAGE_SIZE}\n",
        u64::MAX
    );
    let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::InvalidMeta {
            page_id: Some(1),
            ..
        })
    ));
}

#[test]
#[serial]
fn load_meta_rejects_overlapping_ranges_in_same_file() {
    clear_metafile();
    let half_page = PAGE_SIZE / 2;
    let global_meta = format!(
        "1:\n  file_name: pages.dat\n  offset: 0\n  page_size: {PAGE_SIZE}\n2:\n  file_name: pages.dat\n  offset: {half_page}\n  page_size: {PAGE_SIZE}\n"
    );
    let (_temp, config_path, page_dir) = setup_with_global_meta(&global_meta);
    write_page_file(&page_dir, "pages.dat", PAGE_SIZE * 2);
    set_metafile(&config_path);

    assert!(matches!(
        RawPage::load_meta(),
        Err(PageRuntimeError::InvalidMeta { .. })
    ));
}
