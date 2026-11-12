use super::*;

#[test]
fn walkr_refuses_private_url_parts_and_invalid_probe_paths_without_writing() -> anyhow::Result<()> {
    let temp = TempDirectory::new()?;
    let local = temp.0.join("config-gpui.local.toml");
    let daemon = temp.0.join("absent.toml");
    let before = "# kept\ntheme = \"Nord\"\n";
    fs::write(&local, before)?;
    for (key, value) in [
        ("url", "https://user:password@walkr.test".to_owned()),
        ("url", "https://user@walkr.test".to_owned()),
        ("url", "https://@walkr.test".to_owned()),
        ("url", "https://walkr.test/?token=x".to_owned()),
        ("url", "https://walkr.test/#secret".to_owned()),
        ("url", "file:///tmp/walkr".to_owned()),
        ("probe_path", "sdk.js".to_owned()),
        ("probe_path", "/has space".to_owned()),
        ("probe_path", "/has\ncontrol".to_owned()),
        ("probe_path", format!("/{}", "x".repeat(256))),
    ] {
        let value = toml_edit::Value::from(value);
        assert!(Config::parse(&format!("[walkr]\n{key} = {value}")).is_err());
        assert!(
            Config::edit_key_at(&local, &daemon, &format!("walkr.{key}"), Some(value)).is_err()
        );
        assert_eq!(fs::read_to_string(&local)?, before);
    }
    for (key, value) in [
        ("walkr.url", "https://walkr.test"),
        ("walkr.probe_path", "/sdk.js"),
    ] {
        Config::edit_key_at(&local, &daemon, key, Some(toml_edit::Value::from(value)))?;
    }
    let config = Config::parse(&fs::read_to_string(&local)?)?;
    assert_eq!(
        config
            .walkr
            .url
            .as_ref()
            .map(crate::browser::WebUrl::address)
            .as_deref(),
        Some("walkr.test:443")
    );
    Ok(())
}
