use super::*;

/// Loads `text` as the user's file over the managed defaults, which set a
/// `[tabs] size` and a `tab_layout` of their own.
fn load(text: &str) -> Result<Config> {
    Config::parse_layers([DEFAULT_CONFIG, text], &Daemon::default())
}

#[test]
fn desktop_draws_pills_unless_the_user_sets_a_tab_layout() -> anyhow::Result<()> {
    let desktop = load("layout = 'desktop'\n")?;
    assert_eq!(desktop.tab_layout_in_use(), TabLayoutMode::Pills);
    let classic = load("layout = 'desktop'\ntab_layout = 'classic'\n")?;
    assert_eq!(classic.tab_layout_in_use(), TabLayoutMode::Classic);
    // Every other layout draws the tab layout the files name.
    let normal = load("layout = 'normal'\n")?;
    assert_eq!(normal.tab_layout_in_use(), TabLayoutMode::Classic);
    let cards = load("layout = 'orca'\ntab_layout = 'cards'\n")?;
    assert_eq!(cards.tab_layout_in_use(), TabLayoutMode::Cards);
    Ok(())
}

#[test]
fn desktop_draws_tabs_in_the_ui_face_unless_the_user_sets_the_tabs_face() -> anyhow::Result<()> {
    let ui = "[ui]\nfamily = 'Inter'\nsize = 11\n";
    let desktop = load(&format!("layout = 'desktop'\n{ui}"))?;
    let font = desktop.tabs_font();
    assert_eq!((font.family.as_str(), font.size), ("Inter", 13.));

    let family = load(&format!(
        "layout = 'desktop'\n{ui}[tabs]\nfamily = 'Iosevka'\n"
    ))?;
    let font = family.tabs_font();
    assert_eq!((font.family.as_str(), font.size), ("Iosevka", 13.));

    let size = load(&format!("layout = 'desktop'\n{ui}[tabs]\nsize = 15\n"))?;
    let font = size.tabs_font();
    assert_eq!((font.family.as_str(), font.size), ("Inter", 15.));

    // Glyphs the user's own fallbacks cover stay covered in the `[ui]` face.
    let fallback = load(&format!(
        "layout = 'desktop'\n{ui}[tabs]\nfallback = ['Symbols Nerd Font Mono']\n"
    ))?;
    let font = fallback.tabs_font();
    assert_eq!(font.family, "Inter");
    assert_eq!(
        font.fallbacks.as_deref(),
        Some(&["Symbols Nerd Font Mono".to_owned()][..])
    );

    // Every other layout keeps `[tabs]` whole.
    let normal = load(&format!("layout = 'normal'\n{ui}"))?;
    assert_eq!(*normal.tabs_font(), normal.tabs);
    assert_ne!(normal.tabs.family, "Inter");
    Ok(())
}
