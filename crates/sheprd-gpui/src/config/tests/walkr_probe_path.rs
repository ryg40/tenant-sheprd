use super::*;

#[test]
fn walkr_query_and_fragment_probe_paths_leave_settings_unchanged() -> anyhow::Result<()> {
    let temp = TempDirectory::new()?;
    let local = temp.0.join("config-gpui.local.toml");
    let daemon = temp.0.join("absent.toml");
    let before = "# kept\n[walkr]\nurl = \"https://walkr.test\"\nprobe_path = \"/sdk.js\"\n";
    fs::write(&local, before)?;
    for path in [
        "/sdk.js?token=synthetic",
        "/sdk.js#fragment",
        "/sdk.js?",
        "/sdk.js#",
    ] {
        let value = toml_edit::Value::from(path);
        assert!(Config::parse(&format!("[walkr]\nprobe_path = {value}")).is_err());
        assert!(Config::edit_key_at(&local, &daemon, "walkr.probe_path", Some(value)).is_err());
        assert_eq!(fs::read_to_string(&local)?, before);
    }
    Ok(())
}
