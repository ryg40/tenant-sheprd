use super::*;

/// Loads `text` as the user's file over the managed defaults, which set a
/// `[sidebar] size` of their own.
fn load(text: &str) -> Result<Config> {
    Config::parse_layers([DEFAULT_CONFIG, text], &Daemon::default())
}

#[test]
fn desktop_draws_the_ui_face_unless_the_user_sets_the_sidebar_face() -> anyhow::Result<()> {
    let ui = "[ui]\nfamily = 'Inter'\nsize = 11\n";
    // Spacing keys and the managed default's size are no font choice.
    let desktop = load(&format!("layout = 'desktop'\n{ui}[sidebar]\nindent = 14\n"))?;
    let font = desktop.sidebar_font();
    assert_eq!((font.family.as_str(), font.size), ("Inter", 13.));
    // Without a `[sidebar] fallback`, the `[ui]` face's fallbacks come along.
    assert_eq!(font.fallbacks, desktop.ui.fallbacks);

    let family = load(&format!(
        "layout = 'desktop'\n{ui}[sidebar]\nfamily = 'Iosevka'\n"
    ))?;
    let font = family.sidebar_font();
    assert_eq!((font.family.as_str(), font.size), ("Iosevka", 13.));

    let size = load(&format!("layout = 'desktop'\n{ui}[sidebar]\nsize = 15\n"))?;
    let font = size.sidebar_font();
    assert_eq!((font.family.as_str(), font.size), ("Inter", 15.));

    // A `[sidebar] fallback` of the user's replaces the `[ui]` face's.
    let fallback = load(&format!(
        "layout = 'desktop'\n{ui}[sidebar]\nfallback = ['Symbols Nerd Font Mono']\n"
    ))?;
    let font = fallback.sidebar_font();
    assert_eq!((font.family.as_str(), font.size), ("Inter", 13.));
    assert_eq!(
        font.fallbacks.as_deref(),
        Some(&["Symbols Nerd Font Mono".to_owned()][..])
    );

    // Every other layout keeps `[sidebar]` whole, also after switching away
    // from desktop without a reload.
    let normal = load(&format!("layout = 'normal'\n{ui}"))?;
    assert_eq!(*normal.sidebar_font(), normal.sidebar);
    assert_ne!(normal.sidebar.family, "Inter");
    let mut switched = desktop.clone();
    switched.layout.mode = LayoutMode::default();
    assert_eq!(*switched.sidebar_font(), switched.sidebar);
    Ok(())
}
