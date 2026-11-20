use crate::sidebar::segment_widths;

/// A proportional line too narrow for all its segments keeps each short
/// one whole and divides the rest evenly between the long ones, rather than
/// cutting every segment by the same fraction until a short name is gone.
#[test]
fn short_segments_keep_their_width_while_long_ones_share_the_rest() {
    // A host, a workspace, a tab, and a pane name, as an agent row shows.
    let natural = [18.4, 140.6, 32.2, 190.];
    let available = 220.;
    assert!(natural.iter().sum::<f32>() > available);
    let widths = segment_widths(&natural, available);
    let fair = available / natural.len() as f32;
    let (short, long): (Vec<_>, Vec<_>) = natural
        .iter()
        .zip(&widths)
        .partition(|(natural, _)| **natural <= fair);
    assert!(!short.is_empty() && !long.is_empty());
    for (natural, width) in &short {
        assert!(*width >= *natural, "{natural} cut to {width}");
    }
    for (natural, width) in &long {
        assert!(*width < *natural, "{natural} not cut: {width}");
        assert!(**width > fair, "{width} is no more than an even split");
    }
    let (least, most) = long
        .iter()
        .fold((f32::MAX, 0f32), |(least, most), (_, width)| {
            (least.min(**width), most.max(**width))
        });
    assert!(
        most - least <= 1.,
        "long segments share unevenly: {widths:?}"
    );
    // The segments never pass the end of the line.
    let total: f32 = widths.iter().sum();
    assert!(total <= available, "{widths:?}");
}
