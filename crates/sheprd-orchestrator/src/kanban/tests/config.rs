use super::*;

fn override_for(source: &str) -> SourceOverride {
    SourceOverride {
        in_progress_label: "workflow-active".into(),
        in_review_label: "workflow-review".into(),
        source: source.into(),
    }
}

#[test]
fn invalid_ambiguous_and_hidden_maps_refuse_before_move_preflight() -> Result<()> {
    for label in [
        "".into(),
        "  ".into(),
        "bad,name".into(),
        "bad\nname".into(),
        "x".repeat(crate::kanban::config::MAX_LABEL_BYTES + 1),
        "\u{00e9}".repeat(crate::kanban::config::MAX_LABEL_BYTES),
    ] {
        assert!(matches!(
            Mappings::try_from(Config {
                in_progress_label: label,
                ..Config::default()
            }),
            Err(Error::Label { .. })
        ));
    }
    for label in ["status: in-review", "STATUS: IN-REVIEW"] {
        assert!(matches!(
            Mappings::try_from(Config {
                in_progress_label: label.into(),
                ..Config::default()
            }),
            Err(Error::IdenticalLabels)
        ));
    }
    let selector = "forgejo:forgejo.example.test:team/app";
    for duplicate in [selector, "forgejo:FORGEJO.EXAMPLE.TEST:Team/App"] {
        assert!(matches!(
            Mappings::try_from(Config {
                sources: vec![override_for(selector), override_for(duplicate)],
                ..Config::default()
            }),
            Err(Error::DuplicateSource)
        ));
    }
    let github_selector = "github:github.example.test:Team/App";
    for duplicate in [
        "github:github.example.test:team/app",
        "github:GITHUB.EXAMPLE.TEST:team/app",
    ] {
        assert!(matches!(
            Mappings::try_from(Config {
                sources: vec![override_for(github_selector), override_for(duplicate)],
                ..Config::default()
            }),
            Err(Error::DuplicateSource)
        ));
        let mappings = Mappings::try_from(Config {
            sources: vec![override_for(github_selector)],
            ..Config::default()
        })?;
        let selected: SourceKey = github_selector
            .parse()
            .map_err(|error| Error::Selector(Box::new(error)))?;
        let alias: SourceKey = duplicate
            .parse()
            .map_err(|error| Error::Selector(Box::new(error)))?;
        assert_eq!(
            mappings.for_source(&selected)?.in_progress_label(),
            "workflow-active"
        );
        assert_eq!(
            mappings.for_source(&alias)?,
            Mappings::default().for_source(&alias)?
        );
    }
    assert!(matches!(
        Mappings::try_from(Config {
            sources: (0..=crate::kanban::config::MAX_SOURCE_OVERRIDES)
                .map(|_| override_for(selector))
                .collect(),
            ..Config::default()
        }),
        Err(Error::TooManySources { .. })
    ));
    for hidden in [Provider::Beads, Provider::Gitlab, Provider::Azure] {
        let source = source(hidden, "forgejo.example.test", "team/app");
        let decoded: SourceKey = source
            .canonical()
            .parse()
            .map_err(|error| Error::Selector(Box::new(error)))?;
        assert_eq!(decoded, source);
        assert!(
            matches!(LaneKey::try_from(&source), Err(Error::UnsupportedProvider(provider)) if provider == hidden)
        );
        assert!(
            matches!(Mappings::try_from(Config { sources: vec![override_for(&source.canonical())], ..Config::default() }), Err(Error::UnsupportedProvider(provider)) if provider == hidden)
        );
        let item = issue(&source, "7", "open", &[]);
        let route = checkout(&source, 1);
        let facts = complete();
        let board = project(
            [LaneInput {
                source: &source,
                route: &route,
            }],
            [CardInput {
                item: &item,
                facts: &facts,
                route: &route,
            }],
            &Mappings::default(),
            |_, _| true,
        )?;
        assert!(board.lanes.is_empty());
    }
    for invalid in [
        "forgejo:forgejo.example.test:team/app%oops",
        "forgejo:forgejo.example.test%3a8443:team/app",
    ] {
        assert!(matches!(
            Mappings::try_from(Config {
                sources: vec![override_for(invalid)],
                ..Config::default()
            }),
            Err(Error::SelectorCanonical)
        ));
    }
    for invalid in [
        "forgejo:forgejo.example.test:forgejo.example.test",
        "forgejo:forgejo.example.test:team/",
        "forgejo:forgejo.example.test:team/a,b",
        "forgejo:forgejo.example.test:team/a\nb",
    ] {
        assert!(matches!(
            Mappings::try_from(Config {
                sources: vec![override_for(invalid)],
                ..Config::default()
            }),
            Err(Error::Repository)
        ));
    }
    let Err(error) = Mappings::try_from(Config {
        sources: vec![override_for("forgejo:missing")],
        ..Config::default()
    }) else {
        panic!("invalid selector accepted");
    };
    assert!(std::error::Error::source(&error).is_some());
    assert!(
        matches!(error, Error::Selector(source) if matches!(*source, crate::Error::SourceKey(_)))
    );

    let source = source(
        Provider::Forgejo,
        "forgejo.example.test:8443",
        "team/app%name",
    );
    let item = issue(&source, "7", "open", &[]);
    assert_eq!(
        source
            .canonical()
            .parse::<SourceKey>()
            .map_err(|error| Error::Selector(Box::new(error)))?,
        source
    );
    assert_eq!(
        item.key
            .canonical()
            .parse::<ItemKey>()
            .map_err(|error| Error::Selector(Box::new(error)))?,
        item.key
    );
    let mappings = Mappings::try_from(Config {
        sources: vec![override_for(&source.canonical())],
        ..Config::default()
    })?;
    assert_eq!(
        mappings.for_source(&source)?.in_progress_label(),
        "workflow-active"
    );
    Ok(())
}
