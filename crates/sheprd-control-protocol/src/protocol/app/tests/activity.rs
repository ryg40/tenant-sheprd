use super::*;

#[test]
fn activity_panel_names_round_trip() {
    for &panel in ActivityPanel::ALL {
        assert_eq!(ActivityPanel::parse(panel.name()), Some(panel));
    }
}
