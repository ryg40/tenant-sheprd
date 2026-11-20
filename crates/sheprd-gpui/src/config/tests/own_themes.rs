use super::*;

#[test]
fn the_lavender_and_torch_pairs_resolve_to_a_light_and_a_dark_theme() -> anyhow::Result<()> {
    for (light, dark) in [
        ("Lavender Light", "Lavender Gray"),
        ("Torch Light", "Torch Dark"),
    ] {
        let config = Config {
            theme: ThemeName::system(light, dark),
            ..Config::default()
        };
        for side in [true, false] {
            // No theme directory, so each side loads only as a built-in that
            // parsed. A light theme's ground is lighter than its text, and a
            // dark theme's ground is darker.
            let theme = config.theme_with_directories(side, || Ok(Vec::new()))?;
            let name = ThemeName::side(&config.theme, side);
            let luminance = crate::contrast::luminance;
            assert_eq!(
                luminance(theme.background) > luminance(theme.foreground),
                side,
                "{name}"
            );
        }
    }
    Ok(())
}
